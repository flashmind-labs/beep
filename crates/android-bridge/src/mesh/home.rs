//! What the phone's Home shows: machines, what waits on the user, what runs, open PRs and issues.

use super::Connected;
use crate::{BeepClient, BeepError, bad_request};
use beep_fleet::Desk;
use beep_proto::{
    AgentStatus, GitItem, GitItemKind, GitReview, Machine, MachineSession, MachineStats,
    ProjectSync,
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

/// An open pull request or issue, with the labels the host gave it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileGitItem {
    pub host: String,
    pub repo: String,
    pub number: u64,
    pub kind: GitItemKind,
    pub title: String,
    pub url: String,
    pub labels: Vec<String>,
    pub updated_ms: u64,
    pub authored: bool,
    pub review_requested: bool,
    pub changes_requested: bool,
    pub checks_failing: bool,
    pub draft: bool,
}

/// One machine of the strip.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MachineCell {
    pub id: String,
    pub name: String,
    pub online: bool,
    /// Sessions that have not ended.
    pub agents: u32,
    pub free_bytes: Option<u64>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub seen_ms: u64,
}

/// Something that waits on the user.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum Need {
    Ask {
        session: MachineSession,
    },
    Pull {
        item: MobileGitItem,
    },
    Conflict {
        project: String,
        path: String,
        machines: Vec<String>,
    },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileHome {
    pub machines: Vec<MachineCell>,
    pub needs: Vec<Need>,
    pub running: Vec<MachineSession>,
    pub pulls: Vec<MobileGitItem>,
    pub issues: Vec<MobileGitItem>,
    /// One line for each source that failed; its part is empty.
    pub errors: Vec<String>,
}

/// What the four sources answered.
struct Sources {
    machines: Vec<Machine>,
    stats: Vec<MachineStats>,
    sessions: Vec<MachineSession>,
    items: Result<Vec<MobileGitItem>, String>,
    sync: Result<Vec<ProjectSync>, String>,
}

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    /// Everything Home shows, read from the mesh together.
    pub async fn home(&self) -> Result<MobileHome, BeepError> {
        let Some(mesh) = self.mesh().await? else {
            self.client()?;
            return Err(bad_request("Link a desktop workspace to see Home."));
        };
        Ok(compose(read(&mesh).await))
    }
}

async fn read(mesh: &Arc<Connected>) -> Sources {
    let (machines, stats, sessions, (items, sync)) = tokio::join!(
        mesh.machines(),
        mesh.machine_stats(),
        mesh.sessions(None),
        board(mesh),
    );
    Sources {
        machines,
        stats,
        sessions,
        items,
        sync,
    }
}

#[allow(clippy::type_complexity)]
async fn board(
    mesh: &Arc<Connected>,
) -> (
    Result<Vec<MobileGitItem>, String>,
    Result<Vec<ProjectSync>, String>,
) {
    let desk: Desk = match mesh.desk().await {
        Ok(desk) => desk,
        Err(error) => return (Err(error.to_string()), Err(error.to_string())),
    };
    let (items, sync) = tokio::join!(desk.git_items(), desk.sync());
    let items = items.map_err(|error| error.to_string()).map(|opened| {
        let found = opened.items.found.items;
        found
            .into_iter()
            .zip(
                opened
                    .items
                    .labels
                    .into_iter()
                    .chain(std::iter::repeat(Vec::new())),
            )
            .map(|(item, labels)| mobile_item(item, labels))
            .collect()
    });
    (items, Ok(sync))
}

fn epoch_ms(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_millis()).ok())
        .unwrap_or_default()
}

fn mobile_item(item: GitItem, labels: Vec<String>) -> MobileGitItem {
    MobileGitItem {
        host: item.host,
        repo: item.repo,
        number: item.number,
        kind: item.kind,
        title: item.title,
        url: item.url,
        labels,
        updated_ms: epoch_ms(item.updated_at),
        authored: item.authored,
        review_requested: item.review_requested,
        changes_requested: matches!(item.review, GitReview::ChangesRequested {}),
        checks_failing: item.checks_failing,
        draft: item.draft,
    }
}

/// A pull request asking for the user's review, or one of theirs that needs work.
fn waits_on_you(item: &MobileGitItem) -> bool {
    matches!(item.kind, GitItemKind::PullRequest {})
        && (item.review_requested
            || (item.authored && (item.changes_requested || item.checks_failing)))
}

/// Asks (oldest first), then pull requests, then sync conflicts.
fn needs(sessions: &[MachineSession], pulls: &[MobileGitItem], sync: &[ProjectSync]) -> Vec<Need> {
    let mut asks: Vec<&MachineSession> = sessions
        .iter()
        .filter(|session| session.question.is_some())
        .collect();
    asks.sort_by_key(|session| session.last_activity);
    let mut conflicts = Vec::new();
    for project in sync {
        let mut paths: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for machine in &project.machines {
            for path in &machine.conflicts {
                paths
                    .entry(path)
                    .or_default()
                    .push(machine.machine_name.clone());
            }
        }
        conflicts.extend(paths.into_iter().map(|(path, machines)| Need::Conflict {
            project: project.project.clone(),
            path: path.to_owned(),
            machines,
        }));
    }
    asks.into_iter()
        .map(|session| Need::Ask {
            session: session.clone(),
        })
        .chain(
            pulls
                .iter()
                .filter(|item| waits_on_you(item))
                .map(|item| Need::Pull { item: item.clone() }),
        )
        .chain(conflicts)
        .collect()
}

fn cells(
    machines: Vec<Machine>,
    stats: Vec<MachineStats>,
    live: &[&MachineSession],
) -> Vec<MachineCell> {
    const MB: u64 = 1024 * 1024;
    let mut cells: Vec<MachineCell> = machines
        .into_iter()
        .map(|machine| {
            let stats = stats.iter().find(|stats| stats.id == machine.id);
            MachineCell {
                agents: live
                    .iter()
                    .filter(|session| session.machine_id == machine.id)
                    .count()
                    .try_into()
                    .unwrap_or(u32::MAX),
                free_bytes: stats
                    .and_then(|stats| stats.disks.first())
                    .map(|disk| disk.free_bytes),
                mem_used: stats.map(|stats| stats.memory_used_mb.saturating_mul(MB)),
                mem_total: stats.map(|stats| stats.memory_total_mb.saturating_mul(MB)),
                seen_ms: epoch_ms(machine.seen_at),
                id: machine.id,
                name: machine.name,
                online: machine.online,
            }
        })
        .collect();
    cells.sort_by(|a, b| {
        (!a.online, std::cmp::Reverse(a.agents), &a.name).cmp(&(
            !b.online,
            std::cmp::Reverse(b.agents),
            &b.name,
        ))
    });
    cells
}

fn compose(sources: Sources) -> MobileHome {
    let mut errors = Vec::new();
    let mut items = sources.items.unwrap_or_else(|error| {
        errors.push(format!("Pull requests and issues: {error}"));
        Vec::new()
    });
    let sync = sources.sync.unwrap_or_else(|error| {
        errors.push(format!("Sync: {error}"));
        Vec::new()
    });
    items.sort_by_key(|item| std::cmp::Reverse(item.updated_ms));
    let (pulls, issues): (Vec<_>, Vec<_>) = items
        .into_iter()
        .partition(|item| matches!(item.kind, GitItemKind::PullRequest {}));
    let live: Vec<&MachineSession> = sources
        .sessions
        .iter()
        .filter(|session| !matches!(session.status, AgentStatus::Ended {}))
        .collect();
    MobileHome {
        machines: cells(sources.machines, sources.stats, &live),
        needs: needs(&sources.sessions, &pulls, &sync),
        running: live.into_iter().cloned().collect(),
        pulls,
        issues,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{AgentQuestion, MachineSync, SyncStatus};

    fn at(ms: u64) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_millis(ms)
    }

    fn session(id: &str, asking: bool, last: u64) -> MachineSession {
        MachineSession {
            machine_id: "build-box".into(),
            machine_name: "build-box".into(),
            id: id.into(),
            agent: "codex".into(),
            title: None,
            cwd: "/srv/project".into(),
            status: if asking {
                AgentStatus::Asking {}
            } else {
                AgentStatus::Working {}
            },
            doing: None,
            started_at: at(1),
            last_activity: at(last),
            external: false,
            question: asking.then(|| AgentQuestion {
                id: 1,
                text: "Run cargo test".into(),
                detail: None,
                options: Vec::new(),
            }),
        }
    }

    fn item(number: u64, kind: GitItemKind) -> MobileGitItem {
        MobileGitItem {
            host: "github.com".into(),
            repo: "org/repo".into(),
            number,
            kind,
            title: format!("Item {number}"),
            url: String::new(),
            labels: Vec::new(),
            updated_ms: number,
            authored: false,
            review_requested: false,
            changes_requested: false,
            checks_failing: false,
            draft: false,
        }
    }

    fn pull(number: u64) -> MobileGitItem {
        item(number, GitItemKind::PullRequest {})
    }

    fn conflicted(project: &str, machine: &str, path: &str) -> ProjectSync {
        ProjectSync {
            project: project.into(),
            pinned: false,
            machines: vec![MachineSync {
                machine_id: machine.into(),
                machine_name: machine.into(),
                folder: "/srv/project".into(),
                status: SyncStatus::Failed {},
                files: 0,
                bytes: 0,
                pending: 0,
                conflicts: vec![path.into()],
                rescan_secs: None,
                hint: None,
                error: None,
                git_only: false,
            }],
        }
    }

    fn sources() -> Sources {
        Sources {
            machines: Vec::new(),
            stats: Vec::new(),
            sessions: Vec::new(),
            items: Ok(Vec::new()),
            sync: Ok(Vec::new()),
        }
    }

    #[test]
    fn needs_put_asks_first_oldest_first() {
        let mut waiting = pull(1);
        waiting.review_requested = true;
        let list = needs(
            &[session("late", true, 3000), session("early", true, 1000)],
            &[waiting],
            &[conflicted("app", "laptop", "notes (conflict 1).txt")],
        );
        let order: Vec<String> = list
            .iter()
            .map(|need| match need {
                Need::Ask { session } => session.id.clone(),
                Need::Pull { item } => format!("pull {}", item.number),
                Need::Conflict { path, .. } => format!("conflict {path}"),
            })
            .collect();
        assert_eq!(
            order,
            ["early", "late", "pull 1", "conflict notes (conflict 1).txt"]
        );
    }

    #[test]
    fn pull_waits_on_you_rules() {
        let case = |edit: fn(&mut MobileGitItem)| {
            let mut value = pull(1);
            edit(&mut value);
            waits_on_you(&value)
        };
        assert!(case(|i| i.review_requested = true));
        assert!(case(|i| {
            i.authored = true;
            i.changes_requested = true;
        }));
        assert!(case(|i| {
            i.authored = true;
            i.checks_failing = true;
        }));
        assert!(!case(|i| i.checks_failing = true));
        assert!(!case(|i| i.authored = true));
        let mut issue = item(2, GitItemKind::Issue {});
        issue.review_requested = true;
        assert!(!waits_on_you(&issue));
    }

    #[test]
    fn answered_ask_leaves_needs() {
        let list = needs(&[session("done", false, 1)], &[], &[]);
        assert!(list.is_empty());
    }

    #[test]
    fn one_failing_source_keeps_the_rest() {
        let home = compose(Sources {
            items: Err("host unreachable".into()),
            sessions: vec![session("working", false, 1)],
            sync: Ok(vec![conflicted("app", "laptop", "a.txt")]),
            ..sources()
        });
        assert!(home.pulls.is_empty() && home.issues.is_empty());
        assert_eq!(home.errors.len(), 1);
        assert!(home.errors[0].contains("host unreachable"));
        assert_eq!(home.running.len(), 1);
        assert!(matches!(home.needs[..], [Need::Conflict { .. }]));
    }

    #[test]
    fn machines_sort_online_then_by_agents() {
        let machine = |id: &str, online: bool| Machine {
            id: id.into(),
            name: id.into(),
            online,
            seen_at: at(5),
            version: String::new(),
            env: None,
        };
        let on = |id: &str| MachineSession {
            machine_id: id.into(),
            ..session(id, false, 1)
        };
        let home = compose(Sources {
            machines: vec![
                machine("off-busy", false),
                machine("on-idle", true),
                machine("on-busy", true),
            ],
            sessions: vec![on("off-busy"), on("on-busy")],
            ..sources()
        });
        let order: Vec<&str> = home.machines.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(order, ["on-busy", "on-idle", "off-busy"]);
        assert_eq!(home.machines[0].seen_ms, 5);
    }
}
