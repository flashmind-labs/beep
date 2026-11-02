//! A project's terminals: commands a node keeps running in the background
//! (`process/1`), named and listed per project. The node owns the command
//! and its output; the store only says which task on which node.

use super::{Reply, agent_sessions::linked_keys};
use crate::{
    nodes::Online,
    protocol::{
        node::process::{Reply as TaskReply, Request as TaskRequest, Task, TaskId, TaskState},
        *,
    },
    router::Cx,
    store::TerminalRow,
};
use anyhow::{Result, bail};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// The longest name, command or folder.
const MAX_FIELD: usize = 1000;

pub(super) async fn list_terminals(cx: Cx, request: ListTerminals) -> Reply<Vec<Terminal>> {
    let rows = cx.user.terminals(&request.project).await?;
    let names = machine_names(&cx).await?;
    // Each machine is asked once for all its tasks.
    let mut tasks: HashMap<String, Option<Vec<Task>>> = HashMap::new();
    for endpoint in rows
        .iter()
        .map(|row| row.endpoint.clone())
        .collect::<HashSet<_>>()
    {
        let listed = match online(&cx, &endpoint) {
            Some(node) => match node.tasks(&TaskRequest::List {}).await {
                Ok(TaskReply::Tasks { tasks }) => Some(tasks),
                // A node that cannot list its tasks shows its terminals as offline.
                Ok(_) | Err(_) => None,
            },
            None => None,
        };
        tasks.insert(endpoint, listed);
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let state = match tasks.get(&row.endpoint) {
                Some(Some(listed)) => state_of(&row, listed),
                _ => TerminalState::Offline {},
            };
            view(row, &names, state)
        })
        .collect())
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
        name,
        command: command.to_owned(),
        cwd: cwd.to_owned(),
        started_at: Utc::now(),
    };
    cx.user.add_terminal(&row).await?;
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
        stop(&node, &row)
            .await
            .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", node.name)))?;
    }
    cx.user.remove_terminal(request.id).await?;
    Ok(())
}

/// Stops the row's task when the node still runs it.
async fn stop(node: &Online, row: &TerminalRow) -> Result<()> {
    let id = TaskId(row.task);
    match node.tasks(&TaskRequest::Status { id }).await? {
        TaskReply::Status { task, .. }
            if task.command == row.command && task.state.is_running() =>
        {
            match node.tasks(&TaskRequest::Stop { id }).await? {
                TaskReply::Error { message } => bail!("{message}"),
                _ => Ok(()),
            }
        }
        _ => Ok(()),
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

/// The caller's machine `endpoint` when it is online now.
fn online(cx: &Cx, endpoint: &str) -> Option<Online> {
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

fn state(state: TaskState) -> TerminalState {
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
    fn fields_are_one_line() {
        assert_eq!(field("name", "  logs ").unwrap(), "logs");
        assert!(field("command", "").is_err());
        assert!(field("command", "a\nb").is_err());
        assert!(field("folder", &"x".repeat(MAX_FIELD + 1)).is_err());
    }
}
