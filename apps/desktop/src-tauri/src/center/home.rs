//! Home: machines, what runs, what needs the user, routines coming up,
//! app updates and projects, gathered from the server in one go. Entry
//! point: `center_home`; `section` and `is_unsupported` read any list.

use super::{
    home_sessions::AgentSessions,
    view_cache::{View, ViewCache, cache_view},
};
use crate::{
    beep::{Service, epoch_ms},
    error::{DesktopError, Result},
    fleet::Fleet,
    machines::list as machine_list,
    projects::{ProjectRow, known},
};
use beep_proto::{
    AgentQuestion, AgentStatus, AnswerAgentQuestion, ApiError, AppUpdate, ChatApproval, Cron,
    DismissUpdates, ImportedProject, ListCrons, ListImportedProjects, ListPendingApprovals,
    ListResearches, ListUpdates, Machine, MachineApp, MachineDetails, MachineDisk, MachineEnv,
    MachineGpu, MachineSession, MachineStats, OpenUpdateChat, PendingApproval, ResearchStatus,
    ResearchSummary, Session,
};
use serde::Serialize;
use std::{cmp::Reverse, collections::HashMap, sync::Arc};
use tauri::{AppHandle, State};
use tokio::join;
use uuid::Uuid;

/// Answers a question an agent session asks.
#[tauri::command]
pub async fn center_answer_agent(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    question: u32,
    reply: String,
) -> Result<()> {
    beep.call(AnswerAgentQuestion {
        machine_id,
        agent,
        session,
        question,
        reply,
    })
    .await
}

/// Dismisses updates from the user's apps, by `AppUpdate::id`.
#[tauri::command]
pub async fn center_dismiss_updates(beep: Service<'_>, ids: Vec<i64>) -> Result<()> {
    beep.call(DismissUpdates { ids }).await
}

/// The chat about an update, which opens with Beep telling it; made on the
/// first ask. `None` from a server too old to make one.
#[tauri::command]
pub async fn center_open_update_chat(beep: Service<'_>, id: i64) -> Result<Option<String>> {
    Ok(known(beep.call(OpenUpdateChat { id }).await)?.map(|session| session.id))
}

/// What Home shows. A section is `None` when the server cannot list it for
/// this account.
#[derive(Debug, Serialize)]
pub struct HomeView {
    machines: Option<Vec<MachineCard>>,
    running: Option<Vec<RunningRow>>,
    needs_you: Vec<NeedsYou>,
    coming_up: Option<Vec<CronRow>>,
    updates: Option<Vec<UpdateRow>>,
    /// The projects imported onto the user's machines.
    projects: Option<Vec<ProjectRow>>,
}

/// Something the event triage agent found in one of the user's apps.
#[derive(Debug, Serialize)]
struct UpdateRow {
    id: i64,
    summary: String,
    /// The app's slug, such as `gmail`; empty when unknown.
    app: String,
    /// How the user knows the app, such as "Gmail"; empty when unknown.
    app_name: String,
    /// In milliseconds since the Unix epoch.
    at: u64,
}

impl From<AppUpdate> for UpdateRow {
    fn from(update: AppUpdate) -> Self {
        Self {
            id: update.id,
            summary: update.summary,
            app: update.app,
            app_name: update.app_name,
            at: epoch_ms(update.at),
        }
    }
}

/// A machine with what it reported and what runs on it.
#[derive(Debug, Serialize)]
struct MachineCard {
    id: String,
    name: String,
    online: bool,
    /// In milliseconds since the Unix epoch.
    seen_at: u64,
    version: String,
    env: Option<MachineEnv>,
    apps: Vec<MachineApp>,
    gpus: Vec<MachineGpu>,
    capabilities: Vec<&'static str>,
    /// Agent sessions on it that have not ended.
    agents: u32,
    /// Its live resources; `None` while offline, before its first sample,
    /// or from a server or node from before them.
    stats: Option<CardStats>,
}

/// What a machine last said it has free.
#[derive(Debug, Serialize)]
struct CardStats {
    /// In milliseconds since the Unix epoch.
    sampled_at: u64,
    /// The workspace's volume first, then home's when it is another.
    disks: Vec<MachineDisk>,
    memory_used_mb: u64,
    memory_total_mb: u64,
    load: Option<f64>,
}

impl From<MachineStats> for CardStats {
    fn from(stats: MachineStats) -> Self {
        Self {
            sampled_at: epoch_ms(stats.sampled_at),
            disks: stats.disks,
            memory_used_mb: stats.memory_used_mb,
            memory_total_mb: stats.memory_total_mb,
            load: stats.load,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RunningRow {
    Agent {
        machine_id: String,
        machine_name: String,
        id: String,
        agent: String,
        title: Option<String>,
        cwd: String,
        status: &'static str,
        doing: Option<String>,
        started_at: u64,
        last_activity: u64,
        external: bool,
    },
    Research {
        session: Uuid,
        topic: String,
        status: &'static str,
        round: u32,
        sources: u32,
        started_at: i64,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum NeedsYou {
    Chat {
        session_id: Uuid,
        session_title: String,
        pending: PendingApproval,
    },
    Agent {
        machine_id: String,
        machine_name: String,
        agent: String,
        session: String,
        title: Option<String>,
        question: AgentQuestion,
    },
}

#[derive(Debug, Serialize)]
struct CronRow {
    id: String,
    label: String,
    /// What it asks Beep, the title when it has no label.
    prompt: String,
    schedule: String,
    /// RFC 3339, in UTC.
    next_run_at: String,
}

/// The server's answers, as they came back.
struct HomeParts {
    machines: Result<Vec<Machine>>,
    details: Result<Vec<MachineDetails>>,
    stats: Result<Vec<MachineStats>>,
    sessions: Result<Vec<MachineSession>>,
    researches: Result<Vec<ResearchSummary>>,
    approvals: Result<Vec<ChatApproval>>,
    chats: Result<Vec<Session>>,
    crons: Result<Vec<Cron>>,
    updates: Result<Vec<AppUpdate>>,
    projects: Result<Vec<ImportedProject>>,
}

/// Home: the machines from the mesh, the rest from the server. Every section
/// but the machines and their agent sessions waits for the server; those wait only as long as `AgentSessions` allows.
#[tauri::command]
pub async fn center_home(
    app: AppHandle,
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
    cache: State<'_, Arc<ViewCache>>,
    agents: State<'_, Arc<AgentSessions>>,
) -> Result<HomeView> {
    let (
        machines,
        details,
        stats,
        sessions,
        researches,
        approvals,
        chats,
        crons,
        updates,
        projects,
    ) = join!(
        machine_list::machines(&beep, &fleet),
        machine_list::details(&beep, &fleet),
        machine_list::stats(&beep, &fleet),
        agents.list(&app, beep.inner(), cache.inner()),
        beep.call(ListResearches {}),
        beep.call(ListPendingApprovals {}),
        beep.sessions(),
        beep.call(ListCrons {}),
        beep.call(ListUpdates {}),
        beep.call(ListImportedProjects {}),
    );
    let view = home_view(HomeParts {
        machines: Ok(machines),
        details: Ok(details),
        stats: Ok(stats),
        sessions,
        researches,
        approvals,
        chats,
        crons,
        updates,
        projects,
    })?;
    cache_view(&cache, View::Home, &view);
    Ok(view)
}

/// `None` for a request the server does not know or this account may not
/// make; any other failure goes to the log.
pub(crate) fn section<T>(what: &str, result: Result<T>) -> Option<T> {
    match result {
        Ok(listed) => Some(listed),
        Err(e) if is_unsupported(&e) => None,
        Err(e) => {
            log::warn!("listing {what} for the command center: {e}");
            None
        }
    }
}

/// Home from the server's answers. A failed machine list that is not a
/// plain "not here" means the link itself is down, so the whole Home is
/// stale and the caller keeps what it showed.
fn home_view(parts: HomeParts) -> Result<HomeView> {
    let machines = match parts.machines {
        Err(e) if !is_unsupported(&e) => return Err(e),
        other => other,
    };
    let sessions = section("agent sessions", parts.sessions);
    let live: Vec<&MachineSession> = sessions
        .iter()
        .flatten()
        .filter(|s| !matches!(s.status, AgentStatus::Ended {}))
        .collect();
    let researches = section("researches", parts.researches);
    let running = sessions.as_ref().map(|_| {
        let agents = live.iter().map(|s| RunningRow::from(*s));
        let researching = researches
            .iter()
            .flatten()
            .filter(|r| is_running(r.status))
            .map(RunningRow::from);
        agents.chain(researching).collect()
    });
    let chats = section("chats", parts.chats).unwrap_or_default();
    Ok(HomeView {
        machines: section("machines", machines).map(|machines| {
            let details = section("machine details", parts.details).unwrap_or_default();
            let stats = section("machine stats", parts.stats).unwrap_or_default();
            machine_cards(machines, details, stats, &live)
        }),
        running,
        needs_you: needs_you(
            sessions.iter().flatten(),
            section("approvals", parts.approvals).unwrap_or_default(),
            &chats,
        ),
        coming_up: section("routines", parts.crons).map(coming_up),
        updates: section("updates", parts.updates)
            .map(|updates| updates.into_iter().map(UpdateRow::from).collect()),
        projects: section("projects", parts.projects)
            .map(|projects| projects.into_iter().map(ProjectRow::from).collect()),
    })
}

/// Whether the server does not know the request or this account may not
/// make it, as opposed to a failure on the way.
pub(crate) fn is_unsupported(e: &DesktopError) -> bool {
    matches!(
        e,
        DesktopError::Api(ApiError::BadRequest { .. } | ApiError::Unauthorized {})
    )
}

/// Online machines first, then the busiest, then by name.
fn machine_cards(
    machines: Vec<Machine>,
    details: Vec<MachineDetails>,
    stats: Vec<MachineStats>,
    live: &[&MachineSession],
) -> Vec<MachineCard> {
    let mut agents: HashMap<&str, u32> = HashMap::new();
    for session in live {
        *agents.entry(session.machine_id.as_str()).or_default() += 1;
    }
    let mut details: HashMap<String, MachineDetails> =
        details.into_iter().map(|d| (d.id.clone(), d)).collect();
    let mut stats: HashMap<String, MachineStats> =
        stats.into_iter().map(|s| (s.id.clone(), s)).collect();
    let mut cards: Vec<MachineCard> = machines
        .into_iter()
        .map(|machine| {
            let detail = details.remove(&machine.id);
            MachineCard {
                stats: stats.remove(&machine.id).map(CardStats::from),
                agents: agents.get(machine.id.as_str()).copied().unwrap_or(0),
                seen_at: epoch_ms(machine.seen_at),
                apps: detail.as_ref().map(|d| d.apps.clone()).unwrap_or_default(),
                gpus: detail.as_ref().map(|d| d.gpus.clone()).unwrap_or_default(),
                capabilities: detail
                    .iter()
                    .flat_map(|d| &d.capabilities)
                    .map(|c| <&'static str>::from(*c))
                    .collect(),
                id: machine.id,
                name: machine.name,
                online: machine.online,
                version: machine.version,
                env: machine.env,
            }
        })
        .collect();
    cards.sort_by(|a, b| {
        (Reverse(a.online), Reverse(a.agents), &a.name).cmp(&(
            Reverse(b.online),
            Reverse(b.agents),
            &b.name,
        ))
    });
    cards
}

impl From<&MachineSession> for RunningRow {
    fn from(session: &MachineSession) -> Self {
        Self::Agent {
            machine_id: session.machine_id.clone(),
            machine_name: session.machine_name.clone(),
            id: session.id.clone(),
            agent: session.agent.clone(),
            title: session.title.clone(),
            cwd: session.cwd.clone(),
            status: session.status.into(),
            doing: session.doing.clone(),
            started_at: epoch_ms(session.started_at),
            last_activity: epoch_ms(session.last_activity),
            external: session.external,
        }
    }
}

impl From<&ResearchSummary> for RunningRow {
    fn from(research: &ResearchSummary) -> Self {
        Self::Research {
            session: research.session_id,
            topic: research.topic.clone(),
            status: research.status.as_str(),
            round: research.round,
            sources: research.sources,
            started_at: research.started_at,
        }
    }
}

/// Everything waiting on an answer, oldest first. An agent question counts
/// from its session's last activity. A chat approval carries no time of its
/// own, so it counts from its chat's last change; a chat the list lacks
/// counts as the newest, so it comes last.
pub(crate) fn needs_you<'a>(
    sessions: impl Iterator<Item = &'a MachineSession>,
    approvals: Vec<ChatApproval>,
    chats: &[Session],
) -> Vec<NeedsYou> {
    let changed: HashMap<&str, i64> = chats
        .iter()
        .map(|c| (c.id.as_str(), c.updated_at))
        .collect();
    let mut waiting: Vec<(u64, NeedsYou)> = Vec::new();
    for session in sessions {
        if let Some(question) = &session.question {
            waiting.push((
                epoch_ms(session.last_activity),
                NeedsYou::Agent {
                    machine_id: session.machine_id.clone(),
                    machine_name: session.machine_name.clone(),
                    agent: session.agent.clone(),
                    session: session.id.clone(),
                    title: session.title.clone(),
                    question: question.clone(),
                },
            ));
        }
    }
    for approval in approvals {
        let since = changed
            .get(approval.session_id.to_string().as_str())
            .map_or(u64::MAX, |&at| u64::try_from(at).unwrap_or(0));
        waiting.push((
            since,
            NeedsYou::Chat {
                session_id: approval.session_id,
                session_title: approval.session_title,
                pending: approval.pending,
            },
        ));
    }
    waiting.sort_by_key(|(since, _)| *since);
    waiting.into_iter().map(|(_, item)| item).collect()
}

/// Whether a research is still at work.
fn is_running(status: ResearchStatus) -> bool {
    matches!(
        status,
        ResearchStatus::Planning {}
            | ResearchStatus::Researching {}
            | ResearchStatus::Reviewing {}
            | ResearchStatus::Writing {}
    )
}

/// Enabled routines by when they run next.
fn coming_up(crons: Vec<Cron>) -> Vec<CronRow> {
    let mut rows: Vec<CronRow> = crons
        .into_iter()
        .filter(|c| c.enabled)
        .filter_map(|c| {
            Some(CronRow {
                next_run_at: c.next_run_at?,
                id: c.id,
                label: c.label,
                prompt: c.prompt,
                schedule: c.schedule,
            })
        })
        .collect();
    rows.sort_by(|a, b| a.next_run_at.cmp(&b.next_run_at));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::Approval;
    use std::time::{Duration, SystemTime};

    fn at(ms: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    fn machine(id: &str, online: bool) -> Machine {
        Machine {
            id: id.into(),
            name: id.into(),
            online,
            seen_at: at(0),
            version: "0.1.0".into(),
            env: None,
        }
    }

    fn agent(machine: &str, id: &str, last_activity: u64) -> MachineSession {
        MachineSession {
            machine_id: machine.into(),
            machine_name: machine.into(),
            id: id.into(),
            agent: "claude".into(),
            title: None,
            cwd: "/srv/project".into(),
            status: AgentStatus::Working {},
            doing: None,
            started_at: at(0),
            last_activity: at(last_activity),
            external: false,
            question: None,
        }
    }

    fn asking(machine: &str, id: &str, last_activity: u64) -> MachineSession {
        MachineSession {
            question: Some(AgentQuestion {
                id: 1,
                text: "Run cargo test".into(),
                detail: None,
                options: Vec::new(),
            }),
            ..agent(machine, id, last_activity)
        }
    }

    fn approval(session: Uuid) -> ChatApproval {
        ChatApproval {
            session_id: session,
            session_title: "Chat".into(),
            pending: PendingApproval {
                run_id: "run".into(),
                query_id: "query".into(),
                approval: Approval {
                    app: "Fastmail".into(),
                    action: "send email".into(),
                    fields: Vec::new(),
                },
            },
        }
    }

    fn view(parts: HomeParts) -> HomeView {
        home_view(parts).expect("home built")
    }

    fn parts() -> HomeParts {
        HomeParts {
            machines: Ok(Vec::new()),
            details: Ok(Vec::new()),
            stats: Ok(Vec::new()),
            sessions: Ok(Vec::new()),
            researches: Ok(Vec::new()),
            approvals: Ok(Vec::new()),
            chats: Ok(Vec::new()),
            crons: Ok(Vec::new()),
            updates: Ok(Vec::new()),
            projects: Ok(Vec::new()),
        }
    }

    #[test]
    fn imported_projects_list_and_an_old_server_hides_them() {
        let listed = view(HomeParts {
            projects: Ok(vec![ImportedProject {
                project: "org/repo".into(),
                url: "https://github.com/org/repo".into(),
                machine_id: "m1".into(),
                machine_name: "build-box".into(),
                folder: "/srv/work/projects/org/repo".into(),
                cloned: false,
                imported_at: at(9000),
            }]),
            ..parts()
        });
        let projects = listed.projects.expect("projects were listed");
        assert_eq!(projects.len(), 1);
        let old = view(HomeParts {
            projects: Err(DesktopError::Api(ApiError::bad_request("unknown op 167"))),
            ..parts()
        });
        assert!(old.projects.is_none());
    }

    #[test]
    fn updates_come_with_their_app_and_time() {
        let listed = view(HomeParts {
            updates: Ok(vec![AppUpdate {
                id: 7,
                summary: "The build is ready.".into(),
                app: "gmail".into(),
                app_name: "Gmail".into(),
                at: at(5000),
            }]),
            ..parts()
        });
        let updates = listed.updates.expect("updates were listed");
        assert_eq!(updates.len(), 1);
        assert_eq!((updates[0].id, updates[0].at), (7, 5000));
        assert_eq!(updates[0].app_name, "Gmail");
        let old = view(HomeParts {
            updates: Err(DesktopError::Api(ApiError::bad_request("unknown op 160"))),
            ..parts()
        });
        assert!(old.updates.is_none(), "a server without updates hides them");
    }

    #[test]
    fn machines_count_their_agents() {
        let view = view(HomeParts {
            machines: Ok(vec![machine("build-box", true), machine("mac-box", true)]),
            sessions: Ok(vec![agent("build-box", "a", 1), agent("build-box", "b", 2)]),
            ..parts()
        });
        let cards = view.machines.expect("machines were listed");
        let build = cards.iter().find(|c| c.id == "build-box").expect("card");
        let mac = cards.iter().find(|c| c.id == "mac-box").expect("card");
        assert_eq!((build.agents, mac.agents), (2, 0));
    }

    #[test]
    fn machines_carry_their_stats() {
        let listed = view(HomeParts {
            machines: Ok(vec![machine("build-box", true), machine("mac-box", true)]),
            stats: Ok(vec![MachineStats {
                id: "build-box".into(),
                sampled_at: at(9000),
                disks: vec![MachineDisk {
                    path: "/srv/project".into(),
                    free_bytes: 212_000_000_000,
                    total_bytes: 500_000_000_000,
                }],
                memory_used_mb: 9300,
                memory_total_mb: 32768,
                load: Some(1.5),
            }]),
            ..parts()
        });
        let cards = listed.machines.expect("machines were listed");
        let build = cards.iter().find(|c| c.id == "build-box").expect("card");
        let stats = build.stats.as_ref().expect("stats");
        assert_eq!((stats.sampled_at, stats.memory_used_mb), (9000, 9300));
        assert!(cards.iter().any(|c| c.id == "mac-box" && c.stats.is_none()));
        let old = view(HomeParts {
            machines: Ok(vec![machine("build-box", true)]),
            stats: Err(DesktopError::Api(ApiError::bad_request("unknown op 165"))),
            ..parts()
        });
        assert!(
            old.machines.expect("machines were listed")[0]
                .stats
                .is_none()
        );
    }

    #[test]
    fn unsupported_section_is_none() {
        let view = view(HomeParts {
            machines: Ok(vec![machine("build-box", true)]),
            sessions: Err(DesktopError::Api(ApiError::bad_request("unknown op 131"))),
            ..parts()
        });
        assert!(view.running.is_none());
        assert!(view.machines.is_some());
        assert!(view.coming_up.is_some());
    }

    #[test]
    fn machine_list_failure_fails_home() {
        let failed = home_view(HomeParts {
            machines: Err(DesktopError::Other(anyhow::anyhow!("link is down"))),
            ..parts()
        });
        assert!(failed.is_err());
        let unsupported = home_view(HomeParts {
            machines: Err(DesktopError::Api(ApiError::bad_request("unknown op 130"))),
            ..parts()
        })
        .expect("an old server is not a failure");
        assert!(unsupported.machines.is_none());
    }

    #[test]
    fn machines_sort_online_then_by_agents() {
        let view = view(HomeParts {
            machines: Ok(vec![
                machine("off-busy", false),
                machine("on-idle", true),
                machine("on-busy", true),
                machine("off-idle", false),
            ]),
            sessions: Ok(vec![
                agent("off-busy", "a", 1),
                agent("off-busy", "b", 2),
                agent("on-busy", "c", 3),
            ]),
            ..parts()
        });
        let order: Vec<String> = view
            .machines
            .expect("machines were listed")
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(order, ["on-busy", "on-idle", "off-busy", "off-idle"]);
    }

    #[test]
    fn needs_you_puts_oldest_first() {
        let chat = Uuid::new_v4();
        let view = view(HomeParts {
            sessions: Ok(vec![
                asking("build-box", "late", 3000),
                asking("build-box", "early", 1000),
            ]),
            approvals: Ok(vec![approval(chat)]),
            chats: Ok(vec![Session {
                id: chat.to_string(),
                title: "Chat".into(),
                pinned: false,
                updated_at: 2000,
            }]),
            ..parts()
        });
        let order: Vec<String> = view
            .needs_you
            .iter()
            .map(|item| match item {
                NeedsYou::Chat { .. } => "chat".into(),
                NeedsYou::Agent { session, .. } => session.clone(),
            })
            .collect();
        assert_eq!(order, ["early", "chat", "late"]);
    }

    #[test]
    fn approval_of_a_missing_chat_comes_last() {
        let view = view(HomeParts {
            sessions: Ok(vec![asking("build-box", "agent", 5000)]),
            approvals: Ok(vec![approval(Uuid::new_v4())]),
            ..parts()
        });
        assert!(matches!(view.needs_you[0], NeedsYou::Agent { .. }));
        assert!(matches!(view.needs_you[1], NeedsYou::Chat { .. }));
    }
}
