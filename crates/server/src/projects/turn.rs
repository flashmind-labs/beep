//! A project chat's turn: its prompt, what kind of turn it is, and the
//! context block that tells the agent where the project stands.

use super::{
    commands::{HostCommand, OwnedIssue, parse_owned_issues},
    fleet::{self, Fleet, Machine, NodeFleet},
    issues,
    model::{IssueHost, MachinePath, Visibility},
    personas,
};
use crate::{
    app::{App, Message},
    protocol::node::files::{FileKind, Reply, Request},
    store::{CheckoutRow, ProjectRow, TaskRow},
    user::User,
};
use std::{fmt::Write, sync::Arc, time::Duration};
use tokio::time::timeout;

pub(crate) const PROJECT_PROMPT: &str = include_str!("../../prompts/project/agent.md");

/// The labels of the turns the project posts in its own chat.
pub(crate) const REPLAN_LABEL: &str = "Project re-plan";
pub(crate) const CHECK_LABEL: &str = "Project check";
pub(crate) const WORKER_LABEL: &str = "Project worker";

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
    if message.is_prompt() || message.scheduled.as_deref() == Some(REPLAN_LABEL) {
        TurnKind::Planning
    } else {
        TurnKind::Routine
    }
}

/// The start of each project turn: files the issues earlier tries missed,
/// then gives the context block.
pub(crate) async fn start(app: &Arc<App>, user: &Arc<User>, project: &ProjectRow) -> String {
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    match issues::mirror_missing(&fleet, user, project).await {
        Ok(0) => {}
        Ok(filed) => log::info!("projects: filed {filed} missing issues"),
        Err(error) => log::warn!("projects: unable to file missing issues: {error:#}"),
    }
    context_with(&fleet, user, project).await
}

/// What the agent is told about its project each turn.
pub(crate) async fn context(app: &Arc<App>, user: &Arc<User>, project: &ProjectRow) -> String {
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    context_with(&fleet, user, project).await
}

fn host_name(host: Option<&IssueHost>) -> String {
    match host {
        None => "none, kept in Beep".to_owned(),
        Some(IssueHost::Github) => "GitHub".to_owned(),
        Some(IssueHost::Forgejo(host)) => format!("Forgejo at {}", host.as_str()),
    }
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

/// The personas in the home checkout's `.agents/`.
async fn persona_lines(ctx: &mut String, fleet: &dyn Fleet, home: &CheckoutRow) {
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
        // No folder, or an offline machine: nothing to say.
        _ => return,
    };
    let mut files: Vec<String> = entries
        .into_iter()
        .filter(|entry| entry.kind == FileKind::File && entry.name.ends_with(".md"))
        .filter(|entry| entry.size <= u64::from(MAX_PERSONA_BYTES))
        .map(|entry| entry.name)
        .collect();
    files.sort();
    files.truncate(MAX_PERSONAS);
    let mut lines = Vec::new();
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
        lines.push(match personas::parse(&file, &text) {
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
            .shell
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
    use crate::{
        app::Draws,
        projects::{
            model::{IssueHost, RepoName},
            testing::FakeFleet,
        },
        protocol::{DeviceKind, node::Capability},
        store::{NewTask, TestDb},
    };
    use uuid::Uuid;

    fn message(text: &str, scheduled: Option<&str>) -> Message {
        Message {
            session_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            message: text.to_owned(),
            cron_id: None,
            scheduled: scheduled.map(str::to_owned),
            always_sent: false,
            woken: None,
            images: Vec::new(),
            files: Vec::new(),
            voice: None,
            spoken: false,
            answer: None,
            model: None,
            thinking: None,
            origin: DeviceKind::Phone {},
            platform: None,
            draws: Draws::default(),
            finished_research: false,
        }
    }

    #[test]
    fn planning_turns_are_prompts_and_replans() {
        assert_eq!(turn_kind(&message("Fix it", None)), TurnKind::Planning);
        assert_eq!(
            turn_kind(&message("The worker ended", Some(REPLAN_LABEL))),
            TurnKind::Planning
        );
        assert_eq!(
            turn_kind(&message("Turn done", Some(WORKER_LABEL))),
            TurnKind::Routine
        );
        assert_eq!(
            turn_kind(&message("Checks changed", Some(CHECK_LABEL))),
            TurnKind::Routine
        );
    }

    #[test]
    fn the_prompt_stays_short() {
        assert!(PROJECT_PROMPT.lines().count() < 60);
    }

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
        let project = user
            .ensure_project(&RepoName::parse("acme/widget").unwrap())
            .await
            .unwrap();
        user.add_checkout(
            project.id,
            &CheckoutRow {
                machine: gpu,
                path: MachinePath::parse("/srv/widget").unwrap(),
                home: true,
            },
        )
        .await
        .unwrap();
        let roles = crate::projects::model::Roles {
            issues: Some(IssueHost::Github),
            prs: Some(IssueHost::Github),
        };
        user.set_project_setup(project.id, &roles, Visibility::Private, "main")
            .await
            .unwrap();
        user.create_tasks(
            project.id,
            &[NewTask {
                title: "Fix the build".into(),
                body: String::new(),
                depends_on: None,
                needs: Vec::new(),
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
    async fn unreadable_issues_take_one_line() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = User::for_test(store, "alice");
        let fleet = FakeFleet::default();
        let project = user
            .ensure_project(&RepoName::parse("acme/widget").unwrap())
            .await
            .unwrap();
        let roles = crate::projects::model::Roles {
            issues: Some(IssueHost::Github),
            prs: None,
        };
        user.set_project_setup(project.id, &roles, Visibility::Public, "main")
            .await
            .unwrap();
        let project = user.project(project.id).await.unwrap().unwrap();
        let ctx = context_with(&fleet, &user, &project).await;
        let lines: Vec<&str> = ctx
            .lines()
            .filter(|line| line.starts_with("Could not read the project's issues"))
            .collect();
        assert_eq!(lines.len(), 1, "{ctx}");
        assert!(ctx.contains("Open tasks: none"));
    }
}
