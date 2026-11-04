//! Terminals kept running on the machines of the mesh. A node knows its
//! tasks by number; which one is which terminal, its name and its project
//! are kept in the mesh, one key per terminal, so every desktop lists the
//! same ones and a crash loses none.

use super::Fleet;
use crate::error::{DesktopError, Result};
use beep_mesh::{
    Key,
    machine::{
        Tasks,
        terminals::{self, Kept, field, optional, state_of},
    },
};
use beep_proto::{
    ApiError, OpenTerminal, StartTerminal, Terminal, TerminalEvent, TerminalOutput, TerminalState,
    node::{
        LinkRx, LinkTx,
        mesh_state::{StateValue, TerminalRecord},
        process::{Reply as TaskReply, Request as TaskRequest, Task, TaskId},
        terminal::Size,
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::timeout;
use uuid::Uuid;

/// The folder shown for a terminal started in its node's home folder.
const HOME: &str = "~";
/// How long one machine gets, all told, to say how its terminals stand.
const MACHINE_LIMIT: Duration = Duration::from_secs(5);
/// Machines asked at once.
const IN_FLIGHT: usize = 4;

/// One terminal a desktop keeps, as the mesh holds it under `terminal/<id>`.
#[derive(Debug, Clone)]
struct Row {
    id: Uuid,
    record: TerminalRecord,
}

impl Row {
    fn kept(&self) -> Kept<'_> {
        Kept {
            task: self.record.task,
            pty: self.record.pty,
            command: &self.record.command,
        }
    }

    fn key(&self) -> Key {
        Key::Terminal(self.id.to_string())
    }
}

/// The terminals the mesh holds, in no order.
fn rows(fleet: &Fleet) -> Vec<Row> {
    let Some(mesh) = fleet.mesh() else {
        return Vec::new();
    };
    mesh.scan("terminal/")
        .into_iter()
        .filter_map(|(key, value)| {
            let Key::Terminal(id) = key else {
                return None;
            };
            Some(Row {
                id: id.parse().ok()?,
                record: TerminalRecord::from_bytes(&value).ok()?,
            })
        })
        .collect()
}

fn find(fleet: &Fleet, id: Uuid) -> Option<Row> {
    rows(fleet).into_iter().find(|row| row.id == id)
}

/// Whether the mesh keeps terminal `id`, rather than a server.
pub(crate) fn has(fleet: &Fleet, id: Uuid) -> bool {
    find(fleet, id).is_some()
}

/// Writes the terminal to the mesh, where every desktop reads it.
async fn save(fleet: &Fleet, row: &Row) -> Result<()> {
    let bytes = row.record.to_bytes().map_err(anyhow::Error::from)?;
    let mesh = fleet.ready().await?;
    Ok(mesh.set(row.key(), Some(bytes)).await?)
}

async fn forget(fleet: &Fleet, id: Uuid) -> Result<()> {
    let mesh = fleet.ready().await?;
    Ok(mesh.set(Key::Terminal(id.to_string()), None).await?)
}

fn not_found() -> DesktopError {
    DesktopError::Api(ApiError::NotFound {})
}

fn view(fleet: &Fleet, row: &Row, state: TerminalState) -> Terminal {
    let record = &row.record;
    Terminal {
        id: row.id,
        machine_name: fleet.name_of(record.machine),
        project: record.project.clone(),
        machine_id: record.machine.to_string(),
        name: record.name.clone(),
        command: record.command.clone(),
        cwd: record.cwd.clone(),
        state,
        started_at: UNIX_EPOCH + Duration::from_millis(record.started_at),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// What a machine said of its tasks and its terminals; `None` for a list it
/// was not asked for or could not give.
#[derive(Default)]
struct Listed {
    tasks: Option<Vec<Task>>,
    ptys: Option<Vec<Task>>,
}

/// Asks one machine for the lists `rows` need, on one client, within `MACHINE_LIMIT`.
async fn ask_machine(fleet: &Fleet, machine: EndpointId, want: &Want) -> Listed {
    let asked = async {
        let Ok(client) = fleet.client(machine).await else {
            return Listed::default();
        };
        let (tasks, ptys) = tokio::join!(
            async {
                if want.tasks {
                    terminals::list_tasks(&client).await.ok()
                } else {
                    None
                }
            },
            async {
                if want.ptys {
                    terminals::list_ptys(&client).await.ok()
                } else {
                    None
                }
            }
        );
        Listed { tasks, ptys }
    };
    timeout(MACHINE_LIMIT, asked).await.unwrap_or_default()
}

/// Which lists a machine's rows need.
#[derive(Default)]
struct Want {
    tasks: bool,
    ptys: bool,
}

/// `rows` with how each stands on its machine now. The machines are asked
/// at once, each on its own limit; a machine that does not answer shows its
/// terminals as offline.
async fn with_states(fleet: &Fleet, rows: Vec<Row>) -> Result<Vec<Terminal>> {
    let mut wants: HashMap<EndpointId, Want> = HashMap::new();
    for row in &rows {
        let want = wants.entry(row.record.machine).or_default();
        if row.record.pty {
            want.ptys = true;
        } else {
            want.tasks = true;
        }
    }
    let asks: Vec<_> = wants
        .iter()
        .map(|(machine, want)| async move {
            let listed = if fleet.member(&machine.to_string()).is_some() {
                ask_machine(fleet, *machine, want).await
            } else {
                Listed::default()
            };
            (*machine, listed)
        })
        .collect();
    let listed: HashMap<EndpointId, Listed> = stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let answer = listed.get(&row.record.machine).and_then(|listed| {
            if row.record.pty {
                listed.ptys.as_ref()
            } else {
                listed.tasks.as_ref()
            }
        });
        let state = match answer {
            Some(tasks) => state_of(&row.kept(), tasks),
            None => TerminalState::Offline {},
        };
        // The node drops an exited interactive terminal, so its row goes too.
        if terminals::ended_for_good(&row.kept(), &state) {
            forget(fleet, row.id).await?;
            continue;
        }
        out.push(view(fleet, &row, state));
    }
    Ok(out)
}

/// The terminals on this Mac, as they stand: the node is in this process.
pub(crate) async fn local(fleet: &Fleet) -> Result<Vec<Terminal>> {
    let mine: Vec<Row> = rows(fleet)
        .into_iter()
        .filter(|row| row.record.machine == fleet.id())
        .collect();
    with_states(fleet, mine).await
}

/// The terminals on the other machines of the mesh, as they stand; asks
/// every machine.
pub(crate) async fn remote(fleet: &Fleet) -> Result<Vec<Terminal>> {
    let theirs: Vec<Row> = rows(fleet)
        .into_iter()
        .filter(|row| row.record.machine != fleet.id())
        .collect();
    with_states(fleet, theirs).await
}

/// The terminals of the other machines the mesh holds now, each as `last`
/// had it, or offline when `last` never saw it.
pub(crate) fn remote_now(fleet: &Fleet, last: &[Terminal]) -> Vec<Terminal> {
    rows(fleet)
        .into_iter()
        .filter(|row| row.record.machine != fleet.id())
        .map(|row| {
            last.iter()
                .find(|seen| seen.id == row.id)
                .cloned()
                .unwrap_or_else(|| view(fleet, &row, TerminalState::Offline {}))
        })
        .collect()
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
        record: TerminalRecord {
            project: project.to_owned(),
            machine,
            task: started.0,
            pty: false,
            name,
            command: command.to_owned(),
            cwd: cwd.to_owned(),
            started_at: now_ms(),
        },
    };
    if let Err(e) = save(fleet, &row).await {
        // Not kept, so nobody could stop it later.
        if let Err(stop) = terminals::stop(&client, &row.kept()).await {
            log::warn!("an unsaved terminal was not stopped: {stop:#}");
        }
        return Err(e);
    }
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
        record: TerminalRecord {
            project: project.to_owned(),
            machine,
            task: task.id.0,
            pty: true,
            name,
            // Tells apart a restarted node's terminal under the same number.
            command: task.command,
            cwd: cwd.unwrap_or_else(|| HOME.to_owned()),
            started_at: now_ms(),
        },
    };
    if let Err(e) = save(fleet, &row).await {
        // Not kept, so nobody could stop it later.
        if let Err(stop) = terminals::stop(&client, &row.kept()).await {
            log::warn!("an unsaved terminal was not stopped: {stop:#}");
        }
        return Err(e);
    }
    Ok(view(fleet, &row, TerminalState::Running {}))
}

/// How a terminal stands, with the last lines of a view-only one.
pub(crate) async fn get(fleet: &Fleet, id: Uuid) -> Result<TerminalOutput> {
    let row = find(fleet, id).ok_or_else(not_found)?;
    let offline = |fleet: &Fleet, row: &Row| TerminalOutput {
        terminal: view(fleet, row, TerminalState::Offline {}),
        lines: Vec::new(),
        earlier: false,
    };
    let Some(machine) = fleet.member(&row.record.machine.to_string()) else {
        return Ok(offline(fleet, &row));
    };
    let Ok(client) = fleet.client(machine).await else {
        return Ok(offline(fleet, &row));
    };
    if row.record.pty {
        // Output streams through attach; this only says how it stands.
        let state = match terminals::list_ptys(&client).await {
            Ok(tasks) => state_of(&row.kept(), &tasks),
            Err(_) => TerminalState::Offline {},
        };
        return Ok(TerminalOutput {
            terminal: view(fleet, &row, state),
            lines: Vec::new(),
            earlier: false,
        });
    }
    let status = Tasks(&client)
        .ask(&TaskRequest::Status {
            id: TaskId(row.record.task),
        })
        .await;
    Ok(match status {
        Ok(TaskReply::Status {
            task,
            lines,
            earlier,
        }) if task.command == row.record.command => TerminalOutput {
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
    let row = find(fleet, id).ok_or_else(not_found)?;
    if let Some(machine) = fleet.member(&row.record.machine.to_string())
        && let Ok(client) = fleet.client(machine).await
    {
        terminals::stop(&client, &row.kept())
            .await
            .map_err(|e| DesktopError::BadRequest(format!("{}: {e:#}", fleet.name_of(machine))))?;
    }
    forget(fleet, id).await
}

/// A terminal followed live, or why it cannot be. Held for one attach.
#[allow(clippy::large_enum_variant)]
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
    let row = find(fleet, id).ok_or_else(not_found)?;
    if !row.record.pty {
        return Ok(Opened::Over(TerminalEvent::ViewOnly {}));
    }
    let ended = |state| Ok(Opened::Over(TerminalEvent::Ended { state }));
    let Some(machine) = fleet.member(&row.record.machine.to_string()) else {
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
    let (tx, rx) = terminals::attach(&client, TaskId(row.record.task), from, size)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{}: {e:#}", fleet.name_of(machine))))?;
    Ok(Opened::Live { tx, rx })
}
