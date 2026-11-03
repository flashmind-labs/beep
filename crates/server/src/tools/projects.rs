//! `projects`: the user's projects, where each is checked out, their open
//! pull requests, issues and tasks, and the coding agents working in them.
//! It only reads, so it never asks first.

use crate::{
    api::machine_sessions,
    app::App,
    projects::model::RepoName,
    protocol::{AgentStatus, GitItem, GitItemKind, GitReview, MachineSession},
    store::TaskRow,
    user::User,
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fmt::Write,
    sync::Arc,
    time::{Duration, SystemTime},
};

pub(crate) const NAME: &str = "projects";

/// The line the projects section of a turn's context starts with, also how
/// a chat finds the one it last got.
pub(crate) const HEADER: &str = "The user's projects (`projects` gives their machines, open PRs, \
    issues, tasks and coding sessions):";

/// Projects named in the context and listed by `list`.
const MAX_PROJECTS: usize = 30;
/// Pull requests, issues or tasks `show` lists of each kind.
const MAX_ROWS: usize = 15;
/// Coding sessions listed under one project.
const MAX_SESSIONS: usize = 5;
/// Longest title shown, in characters.
const MAX_TITLE: usize = 90;

pub(crate) struct ProjectsTool {
    pub(crate) app: Arc<App>,
    pub(crate) user: Arc<User>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    action: Action,
    #[serde(default)]
    project: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Action {
    List,
    Show,
}

#[async_trait]
impl Tool for ProjectsTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "The user's projects (git repositories they imported into Beep). `list` gives each \
         project with its URL, the machines and folders it is checked out in, how many open \
         pull requests and issues it has, the PRs waiting on the user, open tasks and the \
         coding-agent sessions running in it. `show` with `project` gives that project's open \
         PRs with review state and checks, issues, tasks and sessions. PRs and issues are the \
         ones that involve the user: opened by them, assigned to them or asking their review. \
         Use it first for questions about the user's projects, priorities, PRs or issues."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "show"]},
                "project": {
                    "type": "string",
                    "description": "For show: the project as org/repo, or just repo when only one has that name."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, args: &Value) -> String {
        match args.get("project").and_then(Value::as_str) {
            Some(project) => format!("Checking {}", project.trim()),
            None => "Checking your projects".to_owned(),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call = ctx.tool_call_id;
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(call, format!("Bad arguments: {e}"))),
        };
        let projects = load(&self.user).await?;
        let now = SystemTime::now();
        let text = match args.action {
            Action::List => {
                if projects.is_empty() {
                    return Ok(ToolResult::success(
                        call,
                        "The user has no projects. They import one from Projects, or you can \
                         with import_project.",
                    ));
                }
                let live = live(&self.app, &self.user).await;
                render_list(&projects, &live, now)
            }
            Action::Show => {
                let Some(wanted) = args.project.as_deref().map(str::trim) else {
                    return Ok(ToolResult::failure(call, "show needs `project`."));
                };
                let project = match find(&projects, wanted) {
                    Ok(project) => project,
                    Err(why) => return Ok(ToolResult::failure(call, why)),
                };
                let live = live(&self.app, &self.user).await;
                render_show(project, &live, now)
            }
        };
        Ok(ToolResult::success(call, text))
    }
}

/// The context's projects section: the names only, so "my projects"
/// resolves without a call. `None` without projects.
pub(crate) fn context_block(names: &[RepoName]) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let shown: Vec<String> = names
        .iter()
        .take(MAX_PROJECTS)
        .map(ToString::to_string)
        .collect();
    let mut block = format!("{HEADER} {}", shown.join(", "));
    if names.len() > MAX_PROJECTS {
        let _ = write!(block, ", and {} more", names.len() - MAX_PROJECTS);
    }
    block.push('.');
    Some(block)
}

/// Where a project is checked out.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    machine: String,
    /// The machine's key, as sessions name it.
    endpoint: String,
    folder: String,
}

/// One project as the store keeps it.
#[derive(Debug)]
struct Project {
    name: RepoName,
    url: Option<String>,
    places: Vec<Place>,
    /// Its tasks not done or dropped.
    tasks: Vec<TaskRow>,
}

/// The user's projects by name, with their places and open tasks.
async fn load(user: &User) -> anyhow::Result<Vec<Project>> {
    let machines: HashMap<String, String> = user
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect();
    let imports = user.imports().await?;
    let mut projects = Vec::new();
    for row in user.projects().await? {
        let name = row.name.to_string();
        let mut url = None;
        let mut places = Vec::new();
        // A machine since unlinked has no name and is left out, as the
        // imported list leaves it out.
        for import in imports
            .iter()
            .filter(|import| import.project.eq_ignore_ascii_case(&name))
        {
            url.get_or_insert_with(|| import.url.clone());
            if let Some(machine) = machines.get(&import.endpoint) {
                places.push(Place {
                    machine: machine.clone(),
                    endpoint: import.endpoint.clone(),
                    folder: import.folder.clone(),
                });
            }
        }
        for checkout in user.checkouts(row.id).await? {
            let endpoint = checkout.machine.to_string();
            let Some(machine) = machines.get(&endpoint) else {
                continue;
            };
            let place = Place {
                machine: machine.clone(),
                endpoint,
                folder: checkout.path.as_str().to_owned(),
            };
            if !places.contains(&place) {
                places.push(place);
            }
        }
        places.sort_by(|a, b| a.machine.cmp(&b.machine).then(a.folder.cmp(&b.folder)));
        let tasks = user
            .project_tasks(row.id)
            .await?
            .into_iter()
            .filter(|task| task.state.is_open())
            .collect();
        projects.push(Project {
            name: row.name,
            url,
            places,
            tasks,
        });
    }
    Ok(projects)
}

/// What the git hosts and machines say now.
#[derive(Debug, Default)]
struct Live {
    items: Vec<GitItem>,
    /// Sessions not ended.
    sessions: Vec<MachineSession>,
    /// What could not be read, for the model.
    notes: Vec<String>,
}

async fn live(app: &App, user: &User) -> Live {
    let (items, sessions) = tokio::join!(git_items(app, user), machine_sessions(app, user));
    let (items, mut notes) = items;
    let sessions = match sessions {
        Ok(sessions) => sessions
            .into_iter()
            .filter(|session| session.status != (AgentStatus::Ended {}))
            .collect(),
        Err(e) => {
            log::warn!("projects tool: sessions not listed: {e:#}");
            notes.push("Coding sessions could not be listed.".to_owned());
            Vec::new()
        }
    };
    Live {
        items,
        sessions,
        notes,
    }
}

/// The user's open items on their git hosts, and notes on the hosts that
/// did not answer.
async fn git_items(app: &App, user: &User) -> (Vec<GitItem>, Vec<String>) {
    let logins = match user.git_logins(&app.credential_key).await {
        Ok(logins) => logins,
        Err(e) => {
            log::warn!("projects tool: git logins not read: {e:#}");
            return (Vec::new(), vec!["PRs and issues could not be read.".into()]);
        }
    };
    if logins.is_empty() {
        return (
            Vec::new(),
            vec![
                "No GitHub or Forgejo token is saved in Settings > Credentials, so PRs and \
                 issues are not listed."
                    .into(),
            ],
        );
    }
    match app.git_items.list(user.id(), logins).await {
        Ok(listed) => {
            let notes = listed
                .hosts
                .iter()
                .filter_map(|host| {
                    let error = host.error.as_deref()?;
                    Some(format!("{} did not answer: {error}", host.host))
                })
                .collect();
            (listed.items, notes)
        }
        Err(e) => {
            log::warn!("projects tool: git items not read: {e:#}");
            (Vec::new(), vec!["PRs and issues could not be read.".into()])
        }
    }
}

/// The project `wanted` names: `org/repo`, or a repo name only one has.
fn find<'a>(projects: &'a [Project], wanted: &str) -> Result<&'a Project, String> {
    let names = || {
        let names: Vec<String> = projects
            .iter()
            .take(MAX_PROJECTS)
            .map(|project| project.name.to_string())
            .collect();
        if names.is_empty() {
            "The user has no projects.".to_owned()
        } else {
            format!("Projects: {}.", names.join(", "))
        }
    };
    if let Some(project) = projects
        .iter()
        .find(|project| project.name.to_string().eq_ignore_ascii_case(wanted))
    {
        return Ok(project);
    }
    let by_repo: Vec<&Project> = projects
        .iter()
        .filter(|project| {
            project
                .name
                .to_string()
                .rsplit('/')
                .next()
                .is_some_and(|repo| repo.eq_ignore_ascii_case(wanted))
        })
        .collect();
    match by_repo[..] {
        [only] => Ok(only),
        [] => Err(format!("No project called {wanted}. {}", names())),
        _ => Err(format!(
            "Several projects are called {wanted}. Give org/repo. {}",
            names()
        )),
    }
}

fn items_of<'a>(project: &Project, live: &'a Live) -> Vec<&'a GitItem> {
    let name = project.name.to_string();
    live.items
        .iter()
        .filter(|item| item.repo.eq_ignore_ascii_case(&name))
        .collect()
}

fn sessions_of<'a>(project: &Project, live: &'a Live) -> Vec<&'a MachineSession> {
    live.sessions
        .iter()
        .filter(|session| in_project(project, session))
        .collect()
}

/// Whether the session runs in one of the project's folders, or under one,
/// as a worktree there does.
fn in_project(project: &Project, session: &MachineSession) -> bool {
    project.places.iter().any(|place| {
        place.endpoint == session.machine_id
            && session
                .cwd
                .strip_prefix(place.folder.trim_end_matches('/'))
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// A pull request that waits on the user: a review asked of them, or their
/// own with changes asked for or failing checks.
fn waits_on_user(item: &GitItem) -> bool {
    item.kind == (GitItemKind::PullRequest {})
        && (item.review_requested
            || (item.authored
                && (item.review == (GitReview::ChangesRequested {}) || item.checks_failing)))
}

fn render_list(projects: &[Project], live: &Live, now: SystemTime) -> String {
    let mut out = format!(
        "{} project{}. PRs and issues count the ones that involve the user (opened, assigned \
         or asking their review).\n",
        projects.len(),
        if projects.len() == 1 { "" } else { "s" }
    );
    for note in &live.notes {
        let _ = writeln!(out, "Note: {note}");
    }
    for project in projects.iter().take(MAX_PROJECTS) {
        let _ = write!(out, "\n- {}", project.name);
        if let Some(url) = &project.url {
            let _ = write!(out, " ({url})");
        }
        out.push('\n');
        let _ = writeln!(out, "  on: {}", places_text(&project.places));
        let items = items_of(project, live);
        let prs = items
            .iter()
            .filter(|item| item.kind == (GitItemKind::PullRequest {}))
            .count();
        let waiting = items.iter().filter(|item| waits_on_user(item)).count();
        let issues = items.len() - prs;
        let _ = write!(out, "  open: {prs} PR{}", plural(prs));
        if waiting > 0 {
            let _ = write!(out, " ({waiting} waiting on the user)");
        }
        let tasks = project.tasks.len();
        let _ = writeln!(
            out,
            ", {issues} issue{}, {tasks} task{}",
            plural(issues),
            plural(tasks)
        );
        let sessions = sessions_of(project, live);
        for session in sessions.iter().take(MAX_SESSIONS) {
            let _ = writeln!(out, "  session: {}", session_text(session, now));
        }
        if sessions.len() > MAX_SESSIONS {
            let _ = writeln!(out, "  and {} more sessions", sessions.len() - MAX_SESSIONS);
        }
    }
    if projects.len() > MAX_PROJECTS {
        let _ = writeln!(
            out,
            "\nAnd {} more projects; `show` any of them by name.",
            projects.len() - MAX_PROJECTS
        );
    }
    let elsewhere = live
        .sessions
        .iter()
        .filter(|session| !projects.iter().any(|project| in_project(project, session)))
        .count();
    if elsewhere > 0 {
        let _ = writeln!(
            out,
            "\n{elsewhere} more coding session{} outside these projects' folders.",
            plural(elsewhere)
        );
    }
    out.trim_end().to_owned()
}

fn render_show(project: &Project, live: &Live, now: SystemTime) -> String {
    let mut out = project.name.to_string();
    if let Some(url) = &project.url {
        let _ = write!(out, " ({url})");
    }
    let _ = writeln!(out, "\nOn: {}", places_text(&project.places));
    for note in &live.notes {
        let _ = writeln!(out, "Note: {note}");
    }
    let items = items_of(project, live);
    let (prs, issues): (Vec<&GitItem>, Vec<&GitItem>) = items
        .into_iter()
        .partition(|item| item.kind == (GitItemKind::PullRequest {}));

    let _ = writeln!(
        out,
        "\nOpen PRs involving the user ({}):",
        count_text(prs.len())
    );
    for pr in prs.iter().take(MAX_ROWS) {
        let _ = writeln!(out, "- {}", pr_text(pr, now));
    }
    let _ = writeln!(
        out,
        "\nOpen issues involving the user ({}):",
        count_text(issues.len())
    );
    for issue in issues.iter().take(MAX_ROWS) {
        let _ = writeln!(out, "- {}", issue_text(issue, now));
    }
    let _ = writeln!(out, "\nOpen tasks ({}):", count_text(project.tasks.len()));
    for task in project.tasks.iter().take(MAX_ROWS) {
        let state: &'static str = task.state.into();
        let _ = write!(
            out,
            "- {} [{state}] {}",
            task.slug.as_str(),
            short(&task.title)
        );
        if let Some(pr) = &task.pr_url {
            let _ = write!(out, ", PR {pr}");
        } else if let Some(issue) = &task.issue {
            let _ = write!(out, ", issue #{}", issue.number);
        }
        out.push('\n');
    }
    let sessions = sessions_of(project, live);
    let _ = writeln!(out, "\nCoding sessions ({}):", count_text(sessions.len()));
    for session in sessions.iter().take(MAX_ROWS) {
        let _ = writeln!(out, "- {}", session_text(session, now));
    }
    out.trim_end().to_owned()
}

/// `none`, a number, or `15 of 40` when the list is cut.
fn count_text(count: usize) -> String {
    match count {
        0 => "none".to_owned(),
        n if n > MAX_ROWS => format!("{MAX_ROWS} of {n}"),
        n => n.to_string(),
    }
}

fn places_text(places: &[Place]) -> String {
    if places.is_empty() {
        return "no machine".to_owned();
    }
    places
        .iter()
        .map(|place| format!("{} {}", place.machine, place.folder))
        .collect::<Vec<_>>()
        .join("; ")
}

fn pr_text(pr: &GitItem, now: SystemTime) -> String {
    let mut facts = Vec::new();
    if pr.draft {
        facts.push("draft".to_owned());
    }
    if pr.authored {
        facts.push("by the user".to_owned());
    }
    if pr.review_requested {
        facts.push("asks the user's review".to_owned());
    }
    match pr.review {
        GitReview::Approved {} => facts.push("approved".to_owned()),
        GitReview::ChangesRequested {} => facts.push("changes requested".to_owned()),
        _ => {}
    }
    if pr.checks_failing {
        facts.push("checks failing".to_owned());
    }
    if pr.comments > 0 {
        facts.push(format!("{} comments", pr.comments));
    }
    facts.push(format!("updated {}", ago(pr.updated_at, now)));
    format!(
        "#{} {} ({}) {}",
        pr.number,
        short(&pr.title),
        facts.join(", "),
        pr.url
    )
}

fn issue_text(issue: &GitItem, now: SystemTime) -> String {
    let mut facts = Vec::new();
    if issue.assigned {
        facts.push("assigned to the user".to_owned());
    }
    if issue.authored {
        facts.push("opened by the user".to_owned());
    }
    if issue.comments > 0 {
        facts.push(format!("{} comments", issue.comments));
    }
    facts.push(format!("updated {}", ago(issue.updated_at, now)));
    format!(
        "#{} {} ({}) {}",
        issue.number,
        short(&issue.title),
        facts.join(", "),
        issue.url
    )
}

/// `claude on build-box "Fix login" (working, 4 min ago) in /srv/app`.
fn session_text(session: &MachineSession, now: SystemTime) -> String {
    let status: &'static str = session.status.into();
    let mut text = format!("{} on {}", session.agent, session.machine_name);
    if let Some(title) = session.title.as_deref().filter(|title| !title.is_empty()) {
        let _ = write!(text, " \"{}\"", short(title));
    }
    let _ = write!(
        text,
        " ({status}, last activity {}) in {}",
        ago(session.last_activity, now),
        session.cwd
    );
    if let Some(doing) = &session.doing {
        let _ = write!(text, ", doing: {}", short(doing));
    }
    if session.question.is_some() {
        text.push_str(", waiting on an answer");
    }
    if session.external {
        text.push_str(", started outside Beep");
    }
    text
}

fn ago(at: SystemTime, now: SystemTime) -> String {
    let minutes = now.duration_since(at).unwrap_or(Duration::ZERO).as_secs() / 60;
    match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..2880 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

fn short(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_TITLE {
        return text.to_owned();
    }
    let cut: String = text.chars().take(MAX_TITLE - 3).collect();
    format!("{}...", cut.trim_end())
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests;
