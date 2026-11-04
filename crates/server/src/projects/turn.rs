//! A project chat's turn: its prompt, its kind and the context block.

use super::{
    commands::{HostCommand, OwnedIssue, parse_owned_issues},
    fleet::{self, Fleet, Machine, NodeFleet},
    issues,
    model::{IssueHost, MachinePath, Visibility},
    personas::{self, PersonaFileError, ProjectPersona},
    threads,
};
use crate::{
    account::user::User,
    app::{App, Message},
    protocol::node::files::{FileKind, Reply, Request},
    store::{CheckoutRow, ProjectRow, TaskRow, ThreadChatRow},
};
use std::{fmt::Write, sync::Arc, time::Duration};
use tokio::time::timeout;

pub(crate) const PROJECT_PROMPT: &str = include_str!("../../prompts/project/agent.md");

/// The labels of the turns the project posts in its own chat.
pub(crate) const REPLAN_LABEL: &str = "Project re-plan";
pub(crate) const CHECK_LABEL: &str = "Project check";
pub(crate) const WORKER_LABEL: &str = "Project worker";
/// The user asked for a plan of an issue on the host.
pub(crate) const ISSUE_LABEL: &str = "Project issue";
/// The user asked for something about a branch review.
pub(crate) const REVIEW_LABEL: &str = "Project review";
/// The user asked for a reply on a pull request or issue.
pub(crate) const REPLY_LABEL: &str = "Project reply";

/// Most project memories the context lists.
const MAX_MEMORIES: usize = 30;
/// Most persona files read from the home checkout, and the largest one.
const MAX_PERSONAS: usize = 32;
const MAX_PERSONA_BYTES: u32 = 64 * 1024;
/// How long reading the owned issues may take.
const ISSUES_LIMIT: Duration = Duration::from_secs(10);
/// Where persona files live in a checkout.
const PERSONAS_DIR: &str = ".agents";

/// Which model a turn runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnKind {
    /// A new request or a re-plan: the stronger planner model.
    Planning,
    /// Worker reports, checks and answers: the chat's default.
    Routine,
}

pub(crate) fn turn_kind(message: &Message) -> TurnKind {
    if message.is_prompt()
        || matches!(
            message.scheduled.as_deref(),
            Some(REPLAN_LABEL | ISSUE_LABEL)
        )
    {
        TurnKind::Planning
    } else {
        TurnKind::Routine
    }
}

/// How long filing missed issues may hold up the start of a turn.
const MIRROR_BUDGET: Duration = Duration::from_secs(30);

/// Each project turn's start: files missed issues, then gives the context block.
pub(crate) async fn start(app: &Arc<App>, user: &Arc<User>, project: &ProjectRow) -> String {
    let fleet = Arc::new(NodeFleet::new(app, user));
    start_with(fleet, user, project, MIRROR_BUDGET).await
}

/// `start` on `fleet`, waiting at most `budget` for missed issues; filing goes on past it.
pub(crate) async fn start_with(
    fleet: Arc<dyn Fleet>,
    user: &Arc<User>,
    project: &ProjectRow,
    budget: Duration,
) -> String {
    let work = {
        let (fleet, user, project) = (fleet.clone(), user.clone(), project.clone());
        async move {
            match issues::mirror_missing(fleet.as_ref(), &user, &project).await {
                Ok(0) => {}
                Ok(filed) => log::info!("projects: filed {filed} missing issues"),
                Err(error) => log::warn!("projects: unable to file missing issues: {error:#}"),
            }
        }
    };
    if !fleet.projects().mirror(project.id, budget, work).await {
        log::info!(
            "projects: filing missing issues takes over {}s; the turn goes on without them",
            budget.as_secs()
        );
    }
    context_with(fleet.as_ref(), user, project).await
}

/// What the agent is told each turn of a thread chat: the thread read again now.
pub(crate) async fn thread_context(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    chat: &ThreadChatRow,
) -> String {
    let fleet = NodeFleet::new(app, user);
    threads::chat_context(&fleet, user, project, chat).await
}

fn host_name(host: Option<&IssueHost>) -> String {
    host.map_or_else(|| "none, kept in Beep".to_owned(), IssueHost::to_string)
}

pub(crate) async fn context_with(fleet: &dyn Fleet, user: &User, project: &ProjectRow) -> String {
    let mut ctx = String::from("## Project\n");
    let _ = writeln!(ctx, "Name: {}", project.name);
    let visibility = match project.visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
    };
    let _ = writeln!(ctx, "Visibility: {visibility}");
    let _ = writeln!(ctx, "Issues: {}", host_name(project.roles.issues.as_ref()));
    let _ = writeln!(
        ctx,
        "Pull requests: {}",
        host_name(project.roles.prs.as_ref())
    );
    let _ = writeln!(ctx, "Default branch: {}", project.default_branch);

    let machines = fleet.machines().await.unwrap_or_else(|error| {
        let _ = writeln!(ctx, "Could not list the machines: {error:#}");
        Vec::new()
    });
    let checkouts = user.checkouts(project.id).await.unwrap_or_else(|error| {
        log::warn!("projects: unable to read checkouts: {error:#}");
        Vec::new()
    });
    if checkouts.is_empty() {
        let _ = writeln!(ctx, "Checkouts: none yet");
    } else {
        let _ = writeln!(ctx, "Checkouts:");
        for checkout in &checkouts {
            let home = if checkout.home { " (home)" } else { "" };
            let _ = writeln!(
                ctx,
                "- {}: {}{home}",
                fleet::name_of(&machines, &checkout.machine),
                checkout.path
            );
        }
    }
    for line in syncthing_lines(&project.name.to_string(), &machines, &checkouts) {
        let _ = writeln!(ctx, "{line}");
    }
    if let Some(pin) = &project.pin {
        let _ = writeln!(ctx, "Pinned to: {}", fleet::name_of(&machines, pin));
    }

    let tasks = user
        .project_tasks(project.id)
        .await
        .unwrap_or_else(|error| {
            log::warn!("projects: unable to read tasks: {error:#}");
            Vec::new()
        });
    let workers = user.live_workers().await.unwrap_or_else(|error| {
        log::warn!("projects: unable to read workers: {error:#}");
        Vec::new()
    });
    let slug_of = |task: &TaskRow| task.slug.to_string();
    if !machines.is_empty() {
        let _ = writeln!(ctx, "Machines:");
        for machine in &machines {
            let running: Vec<String> = workers
                .iter()
                .filter(|worker| worker.machine == machine.id)
                .map(|worker| {
                    tasks
                        .iter()
                        .find(|task| task.id == worker.task)
                        .map_or_else(|| "another project".to_owned(), slug_of)
                })
                .collect();
            let _ = writeln!(ctx, "- {}", machine_line(machine, &running));
        }
    }

    let open: Vec<&TaskRow> = tasks.iter().filter(|task| task.state.is_open()).collect();
    if open.is_empty() {
        let _ = writeln!(ctx, "Open tasks: none");
    } else {
        let _ = writeln!(ctx, "Open tasks:");
        for task in open {
            let state: &'static str = task.state.into();
            let mut line = format!("- {}: {state}, \"{}\"", task.slug, task.title);
            if let Some(after) = task
                .depends_on
                .and_then(|id| tasks.iter().find(|t| t.id == id))
            {
                let _ = write!(line, ", after {}", after.slug);
            }
            if let Some(issue) = &task.issue {
                let _ = write!(line, ", issue #{}", issue.number);
            }
            if let Some(branch) = &task.branch {
                let _ = write!(line, ", branch {branch}");
            }
            if let Some(pr) = &task.pr_url {
                let _ = write!(line, ", PR {pr}");
            }
            if let Some(worker) = workers.iter().find(|worker| worker.task == task.id) {
                let state: &'static str = worker.state.into();
                let _ = write!(
                    line,
                    ", worker {state} on {}",
                    fleet::name_of(&machines, &worker.machine)
                );
            }
            if task.start.is_some() {
                line.push_str(", waiting to start");
            }
            let _ = writeln!(ctx, "{line}");
        }
    }

    memories(&mut ctx, user, project).await;
    if let Some(home) = checkouts.iter().find(|checkout| checkout.home) {
        persona_lines(&mut ctx, fleet, home).await;
    }
    if let Some(host) = &project.roles.issues {
        owned_issues(&mut ctx, fleet, user, project, host).await;
    }
    ctx
}

/// A line per Syncthing-synced checkout naming the machines sharing its folder.
fn syncthing_lines(project: &str, machines: &[Machine], checkouts: &[CheckoutRow]) -> Vec<String> {
    let mut lines = Vec::new();
    for checkout in checkouts {
        let Some(machine) = machines.iter().find(|m| m.id == checkout.machine) else {
            continue;
        };
        let path = checkout.path.as_str();
        let Some(folder) = machine
            .syncthing
            .iter()
            .filter(|synced| synced.holds(path))
            .max_by_key(|synced| synced.path.len())
        else {
            continue;
        };
        let others: Vec<String> = machines
            .iter()
            .filter(|other| other.id != machine.id)
            .filter(|other| other.syncthing.iter().any(|synced| synced.id == folder.id))
            .map(|other| fleet::name_of(machines, &other.id))
            .collect();
        let name = fleet::name_of(machines, &machine.id);
        lines.push(if others.is_empty() {
            format!(
                "{project} on {name} is kept in step by Syncthing with its other devices; \
                 edits there reach them on their own, so don't copy files to them."
            )
        } else {
            format!(
                "{project} on {name} is kept in step by Syncthing with {}; edits there \
                 reach those machines on their own, so don't copy files between them.",
                others.join(", ")
            )
        });
    }
    lines
}

/// `gpu-box (cuda, blender): online, running fix-it`.
fn machine_line(machine: &Machine, running: &[String]) -> String {
    let mut line = fleet::name_of(std::slice::from_ref(machine), &machine.id);
    if !machine.capabilities.is_empty() {
        let names: Vec<&'static str> = machine
            .capabilities
            .iter()
            .map(|&capability| capability.into())
            .collect();
        let _ = write!(line, " ({})", names.join(", "));
    }
    line.push_str(if machine.online {
        ": online"
    } else {
        ": offline"
    });
    if !running.is_empty() {
        let _ = write!(line, ", running {}", running.join(", "));
    }
    line
}

async fn memories(ctx: &mut String, user: &User, project: &ProjectRow) {
    let Ok(scope) = project.name.memory_scope() else {
        return;
    };
    let rows = match user.memory_rows().await {
        Ok(rows) => rows,
        Err(error) => {
            log::warn!("projects: unable to read memories: {error:#}");
            return;
        }
    };
    let mut found = rows
        .iter()
        .filter(|row| row.scope == scope)
        .take(MAX_MEMORIES)
        .peekable();
    if found.peek().is_none() {
        return;
    }
    let _ = writeln!(ctx, "Project memories:");
    for row in found {
        let _ = writeln!(ctx, "- {} ({})", row.content, row.updated());
    }
}

/// The home checkout's `.agents/` personas, or why each does not parse; empty if unreachable.
pub(crate) async fn read_personas(
    fleet: &dyn Fleet,
    home: &CheckoutRow,
) -> Vec<(String, Result<ProjectPersona, PersonaFileError>)> {
    let dir = home.path.join(PERSONAS_DIR);
    let listed = fleet
        .files(
            &home.machine,
            &Request::List {
                path: dir.to_string(),
                limit: 0,
            },
        )
        .await;
    let entries = match listed {
        Ok(Reply::Entries { entries, .. }) => entries,
        // No folder, or an offline machine: nothing to read.
        _ => return Vec::new(),
    };
    let mut files: Vec<String> = entries
        .into_iter()
        .filter(|entry| entry.kind == FileKind::File && entry.name.ends_with(".md"))
        .filter(|entry| entry.size <= u64::from(MAX_PERSONA_BYTES))
        .map(|entry| entry.name)
        .collect();
    files.sort();
    files.truncate(MAX_PERSONAS);
    let mut found = Vec::new();
    for name in files {
        let path: MachinePath = dir.join(&name);
        let read = fleet
            .files(
                &home.machine,
                &Request::Read {
                    path: path.to_string(),
                    offset: 0,
                    limit: MAX_PERSONA_BYTES,
                },
            )
            .await;
        let text = match read {
            Ok(Reply::Data { data, .. }) => String::from_utf8_lossy(&data).into_owned(),
            _ => continue,
        };
        let file = format!("{PERSONAS_DIR}/{name}");
        let parsed = personas::parse(&text);
        found.push((file, parsed));
    }
    found
}

/// The personas in the home checkout's `.agents/`.
async fn persona_lines(ctx: &mut String, fleet: &dyn Fleet, home: &CheckoutRow) {
    let mut lines = Vec::new();
    for (file, parsed) in read_personas(fleet, home).await {
        lines.push(match parsed {
            Ok(found) => {
                let mut line = format!("- {}", found.persona.name);
                if let Some(description) = found.persona.description.as_deref() {
                    let _ = write!(line, ": {}", description.trim());
                }
                if let Some(engine) = found.engine {
                    let _ = write!(line, " (engine {engine})");
                }
                if !found.needs.is_empty() {
                    let needs: Vec<&'static str> =
                        found.needs.iter().map(|&need| need.into()).collect();
                    let _ = write!(line, " (needs {})", needs.join(", "));
                }
                line
            }
            Err(error) => format!("- {file} does not read as a persona: {error}"),
        });
    }
    if !lines.is_empty() {
        let _ = writeln!(ctx, "Personas:");
        for line in lines {
            let _ = writeln!(ctx, "{line}");
        }
    }
}

/// The issues Beep filed, read from the host in one call.
async fn owned_issues(
    ctx: &mut String,
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    host: &IssueHost,
) {
    let read = async {
        let home = fleet::home_shell(fleet, user, project).await?;
        let command = HostCommand::ListOwnedIssues.render(host, &project.name);
        let out = home
            .run(&command, ISSUES_LIMIT)
            .await
            .map_err(|error| format!("{error:#}"))?;
        if out.code != Some(0) {
            return Err(format!(
                "the host command exited with {}",
                out.code
                    .map_or("no code".to_owned(), |code| code.to_string())
            ));
        }
        parse_owned_issues(host, &out.out).map_err(|error| format!("{error:#}"))
    };
    let issues: Vec<OwnedIssue> = match timeout(ISSUES_LIMIT, read).await {
        Ok(Ok(issues)) => issues,
        Ok(Err(why)) => {
            let _ = writeln!(ctx, "Could not read the project's issues: {why}.");
            return;
        }
        Err(_) => {
            let _ = writeln!(
                ctx,
                "Could not read the project's issues: the host took too long."
            );
            return;
        }
    };
    if issues.is_empty() {
        let _ = writeln!(ctx, "Issues Beep filed: none");
        return;
    }
    let _ = writeln!(ctx, "Issues Beep filed:");
    for issue in issues {
        let state = if issue.open { "open" } else { "closed" };
        let _ = writeln!(
            ctx,
            "- #{} {state}: {} ({} comments)",
            issue.number, issue.title, issue.comments
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::testing::widget_project;
    use crate::{
        projects::{
            model::{IssueHost, Roles},
            testing::FakeFleet,
        },
        protocol::node::Capability,
        store::{NewTask, TestDb},
    };

    #[tokio::test]
    async fn context_lists_tasks_machines_and_issues() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = User::for_test(store, "alice");
        let mut fleet = FakeFleet::default();
        let (gpu, shell) = fleet.add(
            1,
            "gpu-box",
            &[Capability::Cuda, Capability::Blender],
            "/srv",
        );
        fleet.files.insert(
            "/srv/widget/.agents/rigger.md".into(),
            "---\nname: rigger\ndescription: Rigs characters\nneeds: [blender]\n---\nRig.\n".into(),
        );
        shell.answer(
            "issue list",
            0,
            r#"[{"number": 7, "state": "OPEN", "title": "Render the intro", "comments": []}]"#,
        );
        let project = widget_project(
            &user,
            gpu,
            Roles {
                issues: Some(IssueHost::Github),
                prs: Some(IssueHost::Github),
            },
            Visibility::Private,
        )
        .await;
        user.create_tasks(
            project.id,
            &[NewTask {
                title: "Fix the build".into(),
                body: String::new(),
                depends_on: None,
                needs: Vec::new(),
                start: None,
            }],
        )
        .await
        .unwrap();
        let project = user.project(project.id).await.unwrap().unwrap();
        let ctx = context_with(&fleet, &user, &project).await;
        assert!(ctx.contains("fix-the-build: planned"), "{ctx}");
        assert!(ctx.contains("gpu-box (cuda, blender): online"), "{ctx}");
        assert!(ctx.contains("#7 open: Render the intro"), "{ctx}");
        assert!(
            ctx.contains("- rigger: Rigs characters (needs blender)"),
            "{ctx}"
        );
        assert!(ctx.contains("- gpu-box: /srv/widget (home)"), "{ctx}");
        assert_eq!(shell.ran_with("issue list").len(), 1);
    }

    #[tokio::test]
    async fn a_running_issue_create_is_never_cut_off() {
        let db = TestDb::new().await.unwrap();
        let user = Arc::new(User::for_test(db.store().await.unwrap(), "alice"));
        let mut fleet = FakeFleet::default();
        let (home, shell) = fleet.add(1, "build-box", &[], "/srv");
        shell.answer(
            "issue create",
            0,
            "https://github.com/acme/widget/issues/3\n",
        );
        let release = shell.block_on("issue create");
        let project = widget_project(
            &user,
            home,
            Roles {
                issues: Some(IssueHost::Github),
                prs: None,
            },
            Visibility::Private,
        )
        .await;
        user.create_tasks(
            project.id,
            &[NewTask {
                title: "Fix the build".into(),
                body: String::new(),
                depends_on: None,
                needs: Vec::new(),
                start: None,
            }],
        )
        .await
        .unwrap();
        let project = user.project(project.id).await.unwrap().unwrap();
        let fleet = Arc::new(fleet);
        // The create is still running when the budget is spent.
        let ctx = start_with(fleet.clone(), &user, &project, Duration::from_millis(10)).await;
        assert!(ctx.contains("fix-the-build: planned"), "{ctx}");
        release.notify_one();
        fleet.projects.mirrored(project.id).await;
        let tasks = user.project_tasks(project.id).await.unwrap();
        assert_eq!(tasks[0].issue.as_ref().map(|issue| issue.number), Some(3));
    }
}
