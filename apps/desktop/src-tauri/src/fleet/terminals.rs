//! Terminals kept running on the machines of the mesh. A node knows its
//! tasks by number; which one is which terminal, its name and its project
//! are kept in the mesh, one key per terminal, so every desktop lists the
//! same ones and a crash loses none.

use super::{
    Fleet,
    node_actor::{FAST_JOB_WAIT, JobKind},
    terminal_rows::{Row, find, forget, rows, save},
};
use crate::{
    beep::epoch_ms,
    error::{DesktopError, Result},
};
use beep_mesh::machine::{
    Tasks,
    terminals::{self, field, optional, state_of},
};
use beep_proto::{
    ApiError, OpenTerminal, Terminal, TerminalEvent, TerminalOutput, TerminalState,
    node::{
        LinkRx, LinkTx,
        events::Value,
        mesh_state::{StateValue, TerminalRecord},
        process::{Reply as TaskReply, Request as TaskRequest, Task, TaskId},
        terminal::{Reply as PtyReply, Size},
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
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
    /// `ptys` is what the machine pushed, not read from it now.
    pushed: bool,
}

/// Asks one machine for the lists `rows` need, on one client, within `MACHINE_LIMIT`. Its
/// terminals are not asked for when it pushed them.
#[tracing::instrument(skip_all, fields(peer = %machine.fmt_short()))]
async fn ask_machine(fleet: &Fleet, machine: EndpointId, want: &Want) -> Listed {
    let pushed = fleet.projections().terminal_pushes.get(machine);
    ask_with(fleet, machine, want, pushed).await
}

/// As `ask_machine`, whatever the machine pushed.
async fn ask_fresh(fleet: &Fleet, machine: EndpointId, want: &Want) -> Listed {
    ask_with(fleet, machine, want, None).await
}

async fn ask_with(
    fleet: &Fleet,
    machine: EndpointId,
    want: &Want,
    pushed: Option<Vec<Task>>,
) -> Listed {
    let want = Want {
        tasks: want.tasks,
        ptys: want.ptys && pushed.is_none(),
    };
    let asked = async {
        if !want.tasks && !want.ptys {
            return Listed {
                ptys: pushed,
                pushed: true,
                ..Listed::default()
            };
        }

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
        let pushed_ptys = pushed.is_some() && ptys.is_none();
        Listed {
            tasks,
            ptys: ptys.or(pushed),
            pushed: pushed_ptys,
        }
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
    let mut listed: HashMap<EndpointId, Listed> = stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;
    let mut out = Vec::with_capacity(rows.len());

    for row in rows {
        let mut state = state_of_row(&row, listed.get(&row.record.machine));

        // A push may come after a terminal was opened, so one that reads as ended is
        // read from its machine before its row goes.
        if terminals::ended_for_good(&row.kept(), &state)
            && listed.get(&row.record.machine).is_some_and(|l| l.pushed)
        {
            let want = Want {
                tasks: false,
                ptys: true,
            };
            let fresh = ask_fresh(fleet, row.record.machine, &want).await;

            if let Some(held) = listed.get_mut(&row.record.machine) {
                held.ptys = fresh.ptys;
                held.pushed = false;
            }

            state = state_of_row(&row, listed.get(&row.record.machine));
        }

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

/// How `row` stands in what its machine listed, offline when it did not answer.
fn state_of_row(row: &Row, listed: Option<&Listed>) -> TerminalState {
    let answer = listed.and_then(|listed| {
        if row.record.pty {
            listed.ptys.as_ref()
        } else {
            listed.tasks.as_ref()
        }
    });

    match answer {
        Some(tasks) => state_of(&row.kept(), tasks),
        None => TerminalState::Offline {},
    }
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

    // A terminal's machine that pushes its list is not read: the screen comes on its own topic.
    if row.record.pty
        && let Some(tasks) = fleet.projections().terminal_pushes.get(machine)
    {
        let state = state_of(&row.kept(), &tasks);

        if !terminals::ended_for_good(&row.kept(), &state) {
            return Ok(bare(view(fleet, &row, state)));
        }
    }

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
