//! Terminals kept running on the machines of the mesh. A node knows its
//! tasks by number; which one is which terminal, its name and its project
//! are kept here, on this Mac, in `terminals.json`.

use super::Fleet;
use crate::error::{DesktopError, Result};
use beep_mesh::{
    PeerClient,
    machine::{
        Tasks,
        terminals::{self, Kept, field, optional, state_of},
    },
};
use beep_proto::{
    ApiError, OpenTerminal, StartTerminal, Terminal, TerminalEvent, TerminalOutput, TerminalState,
    node::{
        LinkRx, LinkTx,
        process::{Reply as TaskReply, Request as TaskRequest, Task, TaskId},
        terminal::Size,
    },
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// The folder shown for a terminal started in its node's home folder.
const HOME: &str = "~";

/// One terminal this Mac keeps.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Row {
    id: Uuid,
    project: String,
    machine: String,
    task: u32,
    pty: bool,
    name: String,
    command: String,
    cwd: String,
    /// Milliseconds since the Unix epoch.
    started_at: u64,
}

impl Row {
    fn kept(&self) -> Kept<'_> {
        Kept {
            task: self.task,
            pty: self.pty,
            command: &self.command,
        }
    }
}

/// The terminals this Mac keeps, saved after every change.
pub(super) struct Store {
    file: PathBuf,
    rows: Mutex<Vec<Row>>,
}

impl Store {
    /// The terminals saved in `dir`; none when there is no file yet or it
    /// cannot be read.
    pub(super) fn load(dir: &Path) -> Self {
        let file = dir.join("terminals.json");
        let rows = fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            file,
            rows: Mutex::new(rows),
        }
    }

    fn all(&self) -> Vec<Row> {
        self.rows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn find(&self, id: Uuid) -> Option<Row> {
        self.all().into_iter().find(|row| row.id == id)
    }

    fn add(&self, row: Row) -> io::Result<()> {
        self.change(|rows| rows.push(row))
    }

    fn remove(&self, id: Uuid) -> io::Result<()> {
        self.change(|rows| rows.retain(|row| row.id != id))
    }

    fn change(&self, edit: impl FnOnce(&mut Vec<Row>)) -> io::Result<()> {
        let mut rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        edit(&mut rows);
        let bytes = serde_json::to_vec(&*rows).map_err(io::Error::other)?;
        fs::write(&self.file, bytes)
    }
}

/// Whether this Mac keeps terminal `id`, rather than a server.
pub(crate) fn has(fleet: &Fleet, id: Uuid) -> bool {
    fleet.terminals.find(id).is_some()
}

fn not_found() -> DesktopError {
    DesktopError::Api(ApiError::NotFound {})
}

fn saved(e: io::Error) -> DesktopError {
    DesktopError::Other(anyhow::Error::new(e).context("the terminal was not saved"))
}

fn view(fleet: &Fleet, row: &Row, state: TerminalState) -> Terminal {
    let name = row
        .machine
        .parse::<EndpointId>()
        .map(|machine| fleet.name_of(machine))
        .unwrap_or_default();
    Terminal {
        id: row.id,
        machine_name: name,
        project: row.project.clone(),
        machine_id: row.machine.clone(),
        name: row.name.clone(),
        command: row.command.clone(),
        cwd: row.cwd.clone(),
        state,
        started_at: UNIX_EPOCH + Duration::from_millis(row.started_at),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// `node`'s background tasks, or terminals when `pty`; `None` shows them as offline.
async fn listed(client: &PeerClient, pty: bool) -> Option<Vec<Task>> {
    if pty {
        terminals::list_ptys(client).await.ok()
    } else {
        terminals::list_tasks(client).await.ok()
    }
}

/// `rows` with how each stands on its machine now.
async fn with_states(fleet: &Fleet, rows: Vec<Row>) -> Result<Vec<Terminal>> {
    // Each machine is asked once for its tasks and once for its terminals.
    let places: HashSet<(String, bool)> = rows
        .iter()
        .map(|row| (row.machine.clone(), row.pty))
        .collect();
    let mut tasks: HashMap<(String, bool), Option<Vec<Task>>> = HashMap::new();
    for (machine, pty) in places {
        let listed = match machine.parse::<EndpointId>() {
            Ok(id) if fleet.member(&machine).is_some() => match fleet.client(id).await {
                Ok(client) => listed(&client, pty).await,
                Err(_) => None,
            },
            _ => None,
        };
        tasks.insert((machine, pty), listed);
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let state = match tasks.get(&(row.machine.clone(), row.pty)) {
            Some(Some(listed)) => state_of(&row.kept(), listed),
            _ => TerminalState::Offline {},
        };
        // The node drops an exited interactive terminal, so its row goes too.
        if terminals::ended_for_good(&row.kept(), &state) {
            fleet.terminals.remove(row.id).map_err(saved)?;
            continue;
        }
        out.push(view(fleet, &row, state));
    }
    Ok(out)
}

pub(crate) async fn list(fleet: &Fleet, project: &str) -> Result<Vec<Terminal>> {
    let rows = fleet
        .terminals
        .all()
        .into_iter()
        .filter(|row| row.project == project)
        .collect();
    with_states(fleet, rows).await
}

pub(crate) async fn list_all(fleet: &Fleet) -> Result<Vec<Terminal>> {
    with_states(fleet, fleet.terminals.all()).await
}

/// Starts a view-only terminal: a background task.
pub(crate) async fn start(fleet: &Fleet, request: StartTerminal) -> Result<Terminal> {
    let command = field("command", &request.command)?;
    let cwd = field("folder", &request.cwd)?;
    let project = field("project", &request.project)?;
    let name = match request.name.trim() {
        "" => command.to_owned(),
        name => field("name", name)?.to_owned(),
    };
    let machine = fleet.member(&request.machine_id).ok_or_else(not_found)?;
    let machine_name = fleet.name_of(machine);
    let client = fleet.client(machine).await?;
    let started = Tasks(&client)
        .start(command, Some(cwd.to_owned()))
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{machine_name}: {e:#}")))?;
    let row = Row {
        id: Uuid::new_v4(),
        project: project.to_owned(),
        machine: request.machine_id,
        task: started.0,
        pty: false,
        name,
        command: command.to_owned(),
        cwd: cwd.to_owned(),
        started_at: now_ms(),
    };
    fleet.terminals.add(row.clone()).map_err(saved)?;
    Ok(view(fleet, &row, TerminalState::Running {}))
}

/// Opens an interactive terminal on a command, or the shell when none.
pub(crate) async fn open(fleet: &Fleet, request: OpenTerminal) -> Result<Terminal> {
    let command = match request.command.trim() {
        "" => None,
        command => Some(field("command", command)?),
    };
    let project = optional("project", &request.project)?;
    let name = match request.name.trim() {
        "" => command.unwrap_or("shell").to_owned(),
        name => field("name", name)?.to_owned(),
    };
    let machine = fleet.member(&request.machine_id).ok_or_else(not_found)?;
    let machine_name = fleet.name_of(machine);
    let client = fleet.client(machine).await?;
    // No folder: the node's own start folder.
    let cwd = Some(optional("folder", &request.cwd)?)
        .filter(|cwd| !cwd.is_empty())
        .map(str::to_owned);
    let task = terminals::open(
        &client,
        command.map(str::to_owned),
        cwd.clone(),
        Size::clamped(request.cols, request.rows),
    )
    .await
    .map_err(|e| DesktopError::BadRequest(format!("{machine_name}: {e:#}")))?;
    let row = Row {
        id: Uuid::new_v4(),
        project: project.to_owned(),
        machine: request.machine_id,
        task: task.id.0,
        pty: true,
        name,
        // Tells apart a restarted node's terminal under the same number.
        command: task.command,
        cwd: cwd.unwrap_or_else(|| HOME.to_owned()),
        started_at: now_ms(),
    };
    if let Err(e) = fleet.terminals.add(row.clone()) {
        // Not kept, so nobody could stop it later.
        if let Err(stop) = terminals::stop(&client, &row.kept()).await {
            log::warn!("an unsaved terminal was not stopped: {stop:#}");
        }
        return Err(saved(e));
    }
    Ok(view(fleet, &row, TerminalState::Running {}))
}

/// How a terminal stands, with the last lines of a view-only one.
pub(crate) async fn get(fleet: &Fleet, id: Uuid) -> Result<TerminalOutput> {
    let row = fleet.terminals.find(id).ok_or_else(not_found)?;
    let offline = |fleet: &Fleet, row: &Row| TerminalOutput {
        terminal: view(fleet, row, TerminalState::Offline {}),
        lines: Vec::new(),
        earlier: false,
    };
    let Some(machine) = fleet.member(&row.machine) else {
        return Ok(offline(fleet, &row));
    };
    let Ok(client) = fleet.client(machine).await else {
        return Ok(offline(fleet, &row));
    };
    if row.pty {
        // Output streams through attach; this only says how it stands.
        let state = match listed(&client, true).await {
            Some(tasks) => state_of(&row.kept(), &tasks),
            None => TerminalState::Offline {},
        };
        return Ok(TerminalOutput {
            terminal: view(fleet, &row, state),
            lines: Vec::new(),
            earlier: false,
        });
    }
    let status = Tasks(&client)
        .ask(&TaskRequest::Status {
            id: TaskId(row.task),
        })
        .await;
    Ok(match status {
        Ok(TaskReply::Status {
            task,
            lines,
            earlier,
        }) if task.command == row.command => TerminalOutput {
            terminal: view(fleet, &row, terminals::state(task.state)),
            lines,
            earlier,
        },
        Ok(_) => TerminalOutput {
            terminal: view(fleet, &row, TerminalState::Gone {}),
            lines: Vec::new(),
            earlier: false,
        },
        Err(e) => {
            return Err(DesktopError::BadRequest(format!(
                "{}: {e:#}",
                fleet.name_of(machine)
            )));
        }
    })
}

/// Stops a terminal and forgets it.
pub(crate) async fn close(fleet: &Fleet, id: Uuid) -> Result<()> {
    let row = fleet.terminals.find(id).ok_or_else(not_found)?;
    if let Some(machine) = fleet.member(&row.machine)
        && let Ok(client) = fleet.client(machine).await
    {
        terminals::stop(&client, &row.kept())
            .await
            .map_err(|e| DesktopError::BadRequest(format!("{}: {e:#}", fleet.name_of(machine))))?;
    }
    fleet.terminals.remove(id).map_err(saved)
}

/// A terminal followed live, or why it cannot be.
pub(crate) enum Opened {
    /// The node's stream: keys go to `tx`, output comes from `rx`.
    Live { tx: LinkTx, rx: LinkRx },
    /// There is nothing to follow: the event says why.
    Over(TerminalEvent),
}

/// Follows terminal `id` from byte `from`, sized `cols` by `rows`.
pub(crate) async fn attach(
    fleet: &Fleet,
    id: Uuid,
    from: u64,
    cols: u32,
    rows: u32,
) -> Result<Opened> {
    let row = fleet.terminals.find(id).ok_or_else(not_found)?;
    if !row.pty {
        return Ok(Opened::Over(TerminalEvent::ViewOnly {}));
    }
    let ended = |state| Ok(Opened::Over(TerminalEvent::Ended { state }));
    let Some(machine) = fleet.member(&row.machine) else {
        return ended(TerminalState::Offline {});
    };
    let Ok(client) = fleet.client(machine).await else {
        return ended(TerminalState::Offline {});
    };
    // A restarted node hands numbers out again.
    let Ok(listed) = terminals::list_ptys(&client).await else {
        return ended(TerminalState::Offline {});
    };
    let now = state_of(&row.kept(), &listed);
    if now == (TerminalState::Gone {}) {
        return ended(now);
    }
    let size = Size::clamped(cols, rows);
    let (tx, rx) = terminals::attach(&client, TaskId(row.task), from, size)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{}: {e:#}", fleet.name_of(machine))))?;
    Ok(Opened::Live { tx, rx })
}
