//! Terminals: commands a node keeps running in the background
//! (`process/1`) or in a pseudo-terminal (`terminal/1`), named and listed
//! per project, or on their own with no project. The node owns the command and its output; the store only
//! says which task or terminal on which node. `attach` follows an
//! interactive one live.

use super::{Reply, agent_sessions::linked_keys};
use crate::{
    nodes::Online,
    protocol::{
        node::{
            process::{Reply as TaskReply, Request as TaskRequest, Task, TaskId, TaskState},
            terminal::{Reply as PtyReply, Request as PtyRequest, Size},
        },
        *,
    },
    router::Cx,
    store::TerminalRow,
};
use anyhow::{Result, bail};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub(super) mod attach;

/// The longest name, command or folder.
const MAX_FIELD: usize = 1000;
/// The folder shown for a terminal started in its node's home folder.
const HOME: &str = "~";

pub(super) async fn list_terminals(cx: Cx, request: ListTerminals) -> Reply<Vec<Terminal>> {
    let rows = cx.user.terminals(&request.project).await?;
    with_states(&cx, rows).await
}

pub(super) async fn list_all_terminals(cx: Cx, _: ListAllTerminals) -> Reply<Vec<Terminal>> {
    let rows = cx.user.all_terminals().await?;
    with_states(&cx, rows).await
}

/// `rows` with how each stands on its machine now.
async fn with_states(cx: &Cx, rows: Vec<TerminalRow>) -> Reply<Vec<Terminal>> {
    let names = machine_names(cx).await?;
    // Each machine is asked once for all its tasks, and once for all its
    // interactive terminals.
    let mut tasks: HashMap<Place, Option<Vec<Task>>> = HashMap::new();
    for place in rows.iter().map(Place::of).collect::<HashSet<_>>() {
        let listed = match online(cx, &place.endpoint) {
            Some(node) => listed(&node, place.pty).await,
            None => None,
        };
        tasks.insert(place, listed);
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let state = match tasks.get(&Place::of(&row)) {
            Some(Some(listed)) => state_of(&row, listed),
            _ => TerminalState::Offline {},
        };
        // An interactive terminal whose shell exited is done: its row goes,
        // as the node drops it too.
        if ended_for_good(&row, &state) {
            cx.user.remove_terminal(row.id).await?;
            continue;
        }
        out.push(view(row, &names, state));
    }
    Ok(out)
}

/// Whether `row` is an interactive terminal that will never run again: it
/// exited, was stopped, or its node no longer has it.
fn ended_for_good(row: &TerminalRow, state: &TerminalState) -> bool {
    row.pty
        && matches!(
            state,
            TerminalState::Exited { .. } | TerminalState::Stopped {} | TerminalState::Gone {}
        )
}

pub(super) async fn start_terminal(cx: Cx, request: StartTerminal) -> Reply<Terminal> {
    let command = field("command", &request.command)?;
    let cwd = field("folder", &request.cwd)?;
    let project = field("project", &request.project)?;
    let name = match request.name.trim() {
        "" => command.to_owned(),
        name => field("name", name)?.to_owned(),
    };
    if !linked_keys(&cx).await?.contains(&request.machine_id) {
        return Err(ApiError::NotFound {});
    }
    let node = online(&cx, &request.machine_id).ok_or(ApiError::NotFound {})?;
    let started = node
        .tasks(&TaskRequest::Start {
            command: command.to_owned(),
            cwd: Some(cwd.to_owned()),
        })
        .await
        .and_then(|reply| match reply {
            TaskReply::Started { id } => Ok(id),
            TaskReply::Error { message } => bail!("{message}"),
            other => bail!("the node answered {other:?}"),
        })
        .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", node.name)))?;
    let row = TerminalRow {
        id: Uuid::new_v4(),
        project: project.to_owned(),
        endpoint: request.machine_id.clone(),
        task: started.0,
        pty: false,
        name,
        command: command.to_owned(),
        cwd: cwd.to_owned(),
        started_at: Utc::now(),
    };
    cx.user.add_terminal(&row).await?;
    let names = HashMap::from([(request.machine_id, node.name.clone())]);
    Ok(view(row, &names, TerminalState::Running {}))
}

pub(super) async fn open_terminal(cx: Cx, request: OpenTerminal) -> Reply<Terminal> {
    let command = match request.command.trim() {
        "" => None,
        command => Some(field("command", command)?),
    };
    let project = optional("project", &request.project)?;
    let name = match request.name.trim() {
        "" => command.unwrap_or("shell").to_owned(),
        name => field("name", name)?.to_owned(),
    };
    if !linked_keys(&cx).await?.contains(&request.machine_id) {
        return Err(ApiError::NotFound {});
    }
    let node = online(&cx, &request.machine_id).ok_or(ApiError::NotFound {})?;
    // No folder: the machine's workspace, else the node's home folder.
    let cwd = match optional("folder", &request.cwd)? {
        "" => node.workspace.clone(),
        cwd => Some(cwd.to_owned()),
    };
    let started = node
        .terminals(&PtyRequest::Start {
            command: command.map(str::to_owned),
            cwd: cwd.clone(),
            size: Size::clamped(request.cols, request.rows),
        })
        .await
        .and_then(|reply| match reply {
            PtyReply::Started { task } => Ok(task),
            PtyReply::Error { message } => bail!("{message}"),
            other => bail!("the node answered {other:?}"),
        })
        .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", node.name)))?;
    let row = TerminalRow {
        id: Uuid::new_v4(),
        project: project.to_owned(),
        endpoint: request.machine_id.clone(),
        task: started.id.0,
        pty: true,
        name,
        // What the node calls it, such as the shell's name, so a restarted
        // node's terminal under the same number is told apart.
        command: started.command,
        cwd: cwd.unwrap_or_else(|| HOME.to_owned()),
        started_at: Utc::now(),
    };
    if let Err(e) = cx.user.add_terminal(&row).await {
        // Not kept, so nobody could stop it later.
        if let Err(stop) = node.stop_terminal(&row).await {
            log::warn!("an unsaved terminal was not stopped: {stop:#}");
        }
        return Err(e.into());
    }
    let names = HashMap::from([(request.machine_id, node.name.clone())]);
    Ok(view(row, &names, TerminalState::Running {}))
}

pub(super) async fn get_terminal(cx: Cx, request: GetTerminal) -> Reply<TerminalOutput> {
    let row = cx
        .user
        .terminal(request.id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let names = machine_names(&cx).await?;
    let Some(node) = online(&cx, &row.endpoint) else {
        return Ok(TerminalOutput {
            terminal: view(row, &names, TerminalState::Offline {}),
            lines: Vec::new(),
            earlier: false,
        });
    };
    if row.pty {
        // Its output streams through `AttachTerminal`; this only says how
        // it stands.
        let state = match listed(&node, true).await {
            Some(tasks) => state_of(&row, &tasks),
            None => TerminalState::Offline {},
        };
        return Ok(TerminalOutput {
            terminal: view(row, &names, state),
            lines: Vec::new(),
            earlier: false,
        });
    }
    let status = node
        .tasks(&TaskRequest::Status {
            id: TaskId(row.task),
        })
        .await;
    Ok(match status {
        Ok(TaskReply::Status {
            task,
            lines,
            earlier,
        }) if task.command == row.command => TerminalOutput {
            terminal: view(row, &names, state(task.state)),
            lines,
            earlier,
        },
        Ok(_) => TerminalOutput {
            terminal: view(row, &names, TerminalState::Gone {}),
            lines: Vec::new(),
            earlier: false,
        },
        Err(e) => return Err(ApiError::bad_request(format!("{}: {e:#}", node.name))),
    })
}

pub(super) async fn close_terminal(cx: Cx, request: CloseTerminal) -> Reply<()> {
    let row = cx
        .user
        .terminal(request.id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    if let Some(node) = online(&cx, &row.endpoint) {
        node.stop_terminal(&row)
            .await
            .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", node.name)))?;
    }
    cx.user.remove_terminal(request.id).await?;
    Ok(())
}

/// Which listing a terminal's state comes from: its node's background
/// tasks, or its interactive terminals.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Place {
    endpoint: String,
    pty: bool,
}

impl Place {
    fn of(row: &TerminalRow) -> Self {
        Self {
            endpoint: row.endpoint.clone(),
            pty: row.pty,
        }
    }
}

/// `node`'s background tasks, or its interactive terminals when `pty`;
/// `None` when it cannot list them, so its terminals show as offline.
async fn listed(node: &Online, pty: bool) -> Option<Vec<Task>> {
    if pty {
        match node.terminals(&PtyRequest::List {}).await {
            Ok(PtyReply::Terminals { terminals }) => Some(terminals),
            Ok(_) | Err(_) => None,
        }
    } else {
        match node.tasks(&TaskRequest::List {}).await {
            Ok(TaskReply::Tasks { tasks }) => Some(tasks),
            Ok(_) | Err(_) => None,
        }
    }
}

/// `text` trimmed, refused when empty, too long or on several lines.
fn field<'a>(what: &str, text: &'a str) -> Result<&'a str, ApiError> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_FIELD || text.contains(['\n', '\r', '\0']) {
        return Err(ApiError::bad_request(format!(
            "A terminal needs a {what} of 1 to {MAX_FIELD} characters on one line."
        )));
    }
    Ok(text)
}

/// `text` trimmed, which may be empty; else as `field` takes it.
fn optional<'a>(what: &str, text: &'a str) -> Result<&'a str, ApiError> {
    match text.trim() {
        "" => Ok(""),
        text => field(what, text),
    }
}

/// The caller's machine `endpoint` when it is online now.
pub(super) fn online(cx: &Cx, endpoint: &str) -> Option<Online> {
    cx.app
        .nodes
        .online(endpoint)
        .filter(|node| node.user == cx.user.id())
}

/// The caller's machines' names, by key.
async fn machine_names(cx: &Cx) -> Result<HashMap<String, String>, ApiError> {
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect())
}

/// How the row's task stands among those its node lists. A task with the
/// same number but another command is a new one after a restart.
fn state_of(row: &TerminalRow, tasks: &[Task]) -> TerminalState {
    tasks
        .iter()
        .find(|task| task.id.0 == row.task && task.command == row.command)
        .map_or(TerminalState::Gone {}, |task| state(task.state))
}

pub(super) fn state(state: TaskState) -> TerminalState {
    match state {
        TaskState::Running => TerminalState::Running {},
        TaskState::Exited { code } => TerminalState::Exited { code },
        TaskState::Stopped => TerminalState::Stopped {},
        _ => TerminalState::Gone {},
    }
}

fn view(row: TerminalRow, names: &HashMap<String, String>, state: TerminalState) -> Terminal {
    Terminal {
        id: row.id,
        machine_name: names.get(&row.endpoint).cloned().unwrap_or_default(),
        project: row.project,
        machine_id: row.endpoint,
        name: row.name,
        command: row.command,
        cwd: row.cwd,
        state,
        started_at: row.started_at.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> TerminalRow {
        TerminalRow {
            id: Uuid::nil(),
            project: "acme/widget".into(),
            endpoint: "node".into(),
            task: 3,
            pty: false,
            name: "dev-server".into(),
            command: "npm run dev".into(),
            cwd: "/srv/project".into(),
            started_at: Utc::now(),
        }
    }

    fn task(id: u32, command: &str, state: TaskState) -> Task {
        Task {
            id: TaskId(id),
            command: command.into(),
            state,
        }
    }

    #[test]
    fn a_terminal_is_its_task_only_while_the_command_matches() {
        let row = row();
        assert_eq!(
            state_of(&row, &[task(3, "npm run dev", TaskState::Running)]),
            TerminalState::Running {}
        );
        assert_eq!(
            state_of(
                &row,
                &[task(3, "npm run dev", TaskState::Exited { code: Some(1) })]
            ),
            TerminalState::Exited { code: Some(1) }
        );
        // The node restarted and gave number 3 to another command.
        assert_eq!(
            state_of(&row, &[task(3, "cargo watch", TaskState::Running)]),
            TerminalState::Gone {}
        );
        assert_eq!(state_of(&row, &[]), TerminalState::Gone {});
    }

    #[test]
    fn only_an_ended_interactive_terminal_is_dropped() {
        let pty = TerminalRow { pty: true, ..row() };
        assert!(ended_for_good(
            &pty,
            &TerminalState::Exited { code: Some(0) }
        ));
        assert!(ended_for_good(&pty, &TerminalState::Stopped {}));
        assert!(ended_for_good(&pty, &TerminalState::Gone {}));
        assert!(!ended_for_good(&pty, &TerminalState::Running {}));
        // Its machine is away: it may still run there.
        assert!(!ended_for_good(&pty, &TerminalState::Offline {}));
        // A background task keeps its last lines to read.
        assert!(!ended_for_good(
            &row(),
            &TerminalState::Exited { code: Some(1) }
        ));
    }

    #[test]
    fn fields_are_one_line() {
        assert_eq!(field("name", "  logs ").unwrap(), "logs");
        assert!(field("command", "").is_err());
        assert!(field("command", "a\nb").is_err());
        assert!(field("folder", &"x".repeat(MAX_FIELD + 1)).is_err());
        assert_eq!(optional("project", "  ").unwrap(), "");
        assert_eq!(optional("project", " acme/widget ").unwrap(), "acme/widget");
        assert!(optional("folder", "a\nb").is_err());
    }
}
