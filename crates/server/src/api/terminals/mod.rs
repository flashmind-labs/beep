//! Terminals: node tasks or pseudo-terminals; the store only says which task on which node.

use super::{
    Reply,
    own_nodes::{linked_keys, machine_names, online},
};
use crate::{
    api::router::Cx,
    nodes::Online,
    protocol::{
        node::{
            process::{Reply as TaskReply, Request as TaskRequest, Task, TaskId},
            terminal::{Reply as PtyReply, Request as PtyRequest, Size},
        },
        *,
    },
    store::TerminalRow,
};
use anyhow::bail;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub(super) mod attach;

pub(super) use beep_mesh::machine::terminals::{self, Kept, field, optional, state};

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
    // Each machine is asked once for its tasks and once for its terminals.
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
        // The node drops an exited interactive terminal, so its row goes too.
        if ended_for_good(&row, &state) {
            cx.user.remove_terminal(row.id).await?;
            continue;
        }
        out.push(view(row, &names, state));
    }
    Ok(out)
}

/// Whether `row` is an interactive terminal that will never run again.
fn ended_for_good(row: &TerminalRow, state: &TerminalState) -> bool {
    terminals::ended_for_good(&kept(row), state)
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
        // Tells apart a restarted node's terminal under the same number.
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
        // Output streams through `AttachTerminal`; this only says how it stands.
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

/// Which listing a terminal's state comes from.
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

/// `node`'s background tasks, or terminals when `pty`; `None` shows them as offline.
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

/// How the row's task stands.
fn state_of(row: &TerminalRow, tasks: &[Task]) -> TerminalState {
    terminals::state_of(&kept(row), tasks)
}

fn kept(row: &TerminalRow) -> Kept<'_> {
    Kept {
        task: row.task,
        pty: row.pty,
        command: &row.command,
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
