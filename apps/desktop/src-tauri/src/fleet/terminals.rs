//! Terminals kept running on the machines of the mesh. A node knows its
//! tasks by number; which one is which terminal, its name and its project
//! are kept in the mesh, one key per terminal, so every desktop lists the
//! same ones and a crash loses none.

use super::{
    Fleet,
    node_actor::{FAST_JOB_WAIT, JobKind},
};
use crate::{
    agents::steer::newer_than,
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::events::Interest,
};
use beep_mesh::{
    Key,
    machine::{
        Tasks,
        terminals::{self, Kept, field, optional, state_of},
    },
};
use beep_proto::{
    ApiError, OpenTerminal, Terminal, TerminalEvent, TerminalOutput, TerminalState,
    node::{
        LinkRx, LinkTx,
        events::{Topic, Value},
        mesh_state::{StateValue, TerminalRecord},
        process::{Reply as TaskReply, Request as TaskRequest, Task, TaskId},
        terminal::{Reply as PtyReply, Size},
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use semver::Version;
use std::{
    collections::HashMap,
    sync::Arc,
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

/// The last released node, which pushes no terminal's screen on a topic of its own: the windows
/// following one are sent it again when that node's terminal or task list changes instead.
const BEFORE_OUTPUT_TOPICS: Version = Version::new(0, 8, 25);

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

/// Writes the terminal to the mesh, where every desktop reads it.
async fn save(fleet: &Fleet, row: &Row) -> Result<()> {
    let bytes = row.record.to_bytes().map_err(anyhow::Error::from)?;
    let mesh = fleet.ready().await?;
    Ok(mesh.set(row.key(), Some(bytes)).await?)
}

pub(super) async fn forget(fleet: &Fleet, id: Uuid) -> Result<()> {
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

/// What a machine said of its tasks and its terminals; `None` for a list it
/// was not asked for or could not give.
#[derive(Default)]
struct Listed {
    tasks: Option<Vec<Task>>,
    ptys: Option<Vec<Task>>,
}

/// Asks one machine for the lists `rows` need, on one client, within `MACHINE_LIMIT`.
#[tracing::instrument(skip_all, fields(peer = %machine.fmt_short()))]
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
                    terminals::list_ptys_fresh(&client).await.ok()
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
            if let Err(e) = forget(fleet, row.id).await {
                log::warn!("could not forget an exited terminal id={}: {e}", row.id);
            }

            continue;
        }

        out.push(view(fleet, &row, state));
    }

    Ok(out)
}

/// The terminals the mesh holds that are on this Mac, or on the others,
/// hiding any whose close is currently queued in `node_ops`.
fn rows_on(fleet: &Fleet, mine: bool) -> Vec<Row> {
    rows(fleet)
        .into_iter()
        .filter(|row| {
            (row.record.machine == fleet.id()) == mine
                && !fleet.node_ops().is_closing_terminal(row.id)
        })
        .collect()
}

/// The terminals on this Mac, as they stand: the node is in this process.
pub(crate) async fn local(fleet: &Fleet) -> Result<Vec<Terminal>> {
    with_states(fleet, rows_on(fleet, true)).await
}

/// The terminals on the other machines of the mesh, as they stand; asks
/// every machine.
pub(crate) async fn remote(fleet: &Fleet) -> Result<Vec<Terminal>> {
    with_states(fleet, rows_on(fleet, false)).await
}

/// The terminals of the other machines the mesh holds now, each as `last`
/// had it, or offline when `last` never saw it.
pub(crate) fn remote_now(fleet: &Fleet, last: &[Terminal]) -> Vec<Terminal> {
    rows_on(fleet, false)
        .into_iter()
        .map(|row| {
            last.iter()
                .find(|seen| seen.id == row.id)
                .cloned()
                .unwrap_or_else(|| view(fleet, &row, TerminalState::Offline {}))
        })
        .collect()
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
            started_at: epoch_ms(SystemTime::now()),
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

/// A terminal with no lines to show.
fn bare(terminal: Terminal) -> TerminalOutput {
    TerminalOutput {
        terminal,
        lines: Vec::new(),
        earlier: false,
    }
}

/// How a terminal stands, with the last lines of a view-only one.
pub(crate) async fn get(fleet: &Fleet, id: Uuid) -> Result<TerminalOutput> {
    let row = find(fleet, id).ok_or_else(not_found)?;
    let offline = |fleet: &Fleet, row: &Row| bare(view(fleet, row, TerminalState::Offline {}));
    let Some(machine) = fleet.member(&row.record.machine.to_string()) else {
        return Ok(offline(fleet, &row));
    };
    let Ok(Ok(client)) = timeout(MACHINE_LIMIT, fleet.client(machine)).await else {
        return Ok(offline(fleet, &row));
    };

    if row.record.pty {
        // Output streams through attach; this only says how it stands.
        let state = match timeout(MACHINE_LIMIT, terminals::list_ptys_fresh(&client)).await {
            Ok(Ok(tasks)) => state_of(&row.kept(), &tasks),
            _ => TerminalState::Offline {},
        };
        return Ok(bare(view(fleet, &row, state)));
    }

    let Ok(status) = timeout(
        MACHINE_LIMIT,
        Tasks(&client).ask(&TaskRequest::Status {
            id: TaskId(row.record.task),
        }),
    )
    .await
    else {
        return Ok(offline(fleet, &row));
    };
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
        Ok(_) => bare(view(fleet, &row, TerminalState::Gone {})),
        Err(e) => {
            return Err(DesktopError::BadRequest(format!(
                "{}: {e:#}",
                fleet.name_of(machine)
            )));
        }
    })
}

/// Stops a terminal through its machine's actor and removes its mesh record once stopped,
/// keeping the record persisted across crashes or refusals while hiding it locally while queued.
#[cfg(test)]
pub(crate) async fn close(fleet: &Arc<Fleet>, id: Uuid) -> Result<()> {
    close_with(None, fleet, id).await
}

pub(crate) async fn close_with(
    app: Option<tauri::AppHandle>,
    fleet: &Arc<Fleet>,
    id: Uuid,
) -> Result<()> {
    let Some(row) = find(fleet, id) else {
        return Ok(());
    };
    let Some(machine) = fleet.member(&row.record.machine.to_string()) else {
        return forget(fleet, id).await;
    };

    fleet
        .node_ops()
        .submit(
            app,
            fleet.clone(),
            machine,
            JobKind::TerminalStop {
                id,
                task: row.record.task,
                pty: row.record.pty,
                command: row.record.command,
            },
            FAST_JOB_WAIT,
        )
        .await?;
    Ok(())
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
    // Listings remove exited shells. A saved tab can outlive its mesh row.
    let Some(row) = find(fleet, id) else {
        return Ok(Opened::Over(TerminalEvent::Ended {
            state: TerminalState::Gone {},
        }));
    };

    if !row.record.pty {
        return Ok(Opened::Over(TerminalEvent::ViewOnly {}));
    }

    let ended = |state| Ok(Opened::Over(TerminalEvent::Ended { state }));
    let Some(machine) = fleet.member(&row.record.machine.to_string()) else {
        return ended(TerminalState::Offline {});
    };
    let Ok(Ok(client)) = timeout(MACHINE_LIMIT, fleet.client(machine)).await else {
        return ended(TerminalState::Offline {});
    };
    // A restarted node hands numbers out again.
    let Ok(Ok(listed)) = timeout(MACHINE_LIMIT, terminals::list_ptys_fresh(&client)).await else {
        return ended(TerminalState::Offline {});
    };
    let now = state_of(&row.kept(), &listed);

    if now == (TerminalState::Gone {}) {
        return ended(now);
    }

    let size = Size::clamped(cols, rows);
    let (tx, rx) = timeout(
        MACHINE_LIMIT,
        terminals::attach(&client, TaskId(row.record.task), from, size),
    )
    .await
    .map_err(|_| {
        DesktopError::TimedOut(format!(
            "{} did not answer in time.",
            fleet.name_of(machine)
        ))
    })?
    .map_err(|e| DesktopError::BadRequest(format!("{}: {e:#}", fleet.name_of(machine))))?;
    Ok(Opened::Live { tx, rx })
}

/// Where a terminal runs: its machine, and its number there among the node's terminals (`pty`)
/// or among its background tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) id: Uuid,
    pub(crate) machine: EndpointId,
    pub(crate) task: u32,
    pub(crate) pty: bool,
}

/// Where each terminal the mesh holds runs.
pub(crate) fn places(fleet: &Fleet) -> Vec<Place> {
    rows(fleet)
        .into_iter()
        .map(|row| Place {
            id: row.id,
            machine: row.record.machine,
            task: row.record.task,
            pty: row.record.pty,
        })
        .collect()
}

/// The terminal a window's topic follows the output of, when it names one.
pub(crate) fn followed(topic: &Topic) -> Option<Uuid> {
    if topic.name() != "terminal-output" || topic.is_pattern() {
        return None;
    }

    topic.key()?.parse().ok()
}

/// Whether `machine` is a node from before the topics of one terminal's screen. This computer's
/// own node is the desktop's build.
pub(crate) fn old_node(fleet: &Fleet, machine: EndpointId) -> bool {
    machine != fleet.me
        && !newer_than(
            fleet.status().version(machine).as_deref(),
            &BEFORE_OUTPUT_TOPICS,
        )
}

/// The topic a node pushes `place`'s screen on, or the list that changes with it on a node from
/// before.
fn node_topic(place: &Place, old: bool) -> Topic {
    match (old, place.pty) {
        (true, true) => Topic::new("terminals"),
        (true, false) => Topic::new("processes"),
        (false, true) => Topic::keyed("terminal-output", place.task.to_string()),
        (false, false) => Topic::keyed("process-output", place.task.to_string()),
    }
}

/// What the nodes are asked for, given what the windows follow: a window follows a terminal by its
/// id, and its node knows it by number, so each such interest becomes the node's topic on the
/// terminal's machine. A terminal the mesh does not hold yet is left out until it does.
pub(crate) fn node_interests(
    interests: Vec<Interest>,
    places: &[Place],
    old: impl Fn(EndpointId) -> bool,
) -> Vec<Interest> {
    interests
        .into_iter()
        .filter_map(|interest| {
            let Some(id) = followed(&interest.topic) else {
                return Some(interest);
            };
            let place = places.iter().find(|place| place.id == id)?;

            Some(Interest {
                machine: Some(place.machine.to_string()),
                topic: node_topic(place, old(place.machine)),
            })
        })
        .collect()
}

/// The screen of terminal `id` as its node pushed it on its topic.
pub(crate) fn pushed(fleet: &Fleet, id: Uuid, value: &Value) -> Result<TerminalOutput> {
    let row = find(fleet, id).ok_or_else(not_found)?;
    let gone = |fleet: &Fleet| bare(view(fleet, &row, TerminalState::Gone {}));

    Ok(match value {
        Value::Terminals(PtyReply::Terminals { terminals }) => {
            bare(view(fleet, &row, state_of(&row.kept(), terminals)))
        }
        Value::Process(TaskReply::Status {
            task,
            lines,
            earlier,
        }) if task.command == row.record.command => TerminalOutput {
            terminal: view(fleet, &row, terminals::state(task.state)),
            lines: lines.clone(),
            earlier: *earlier,
        },
        Value::Missing | Value::Terminals(_) | Value::Process(_) => gone(fleet),
        _ => {
            return Err(DesktopError::BadRequest(
                "a node pushed another value than a terminal's screen".into(),
            ));
        }
    })
}

#[cfg(test)]
#[path = "../tests/fleet_terminals.rs"]
mod tests;
