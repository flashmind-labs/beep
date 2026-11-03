//! Terminals for the window, of a project or on every machine: commands
//! kept running on the user's machines, their output, starting and closing them, and following
//! an interactive one live while typing into it.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
    projects::unknown_op,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AttachTerminal, CloseTerminal, GetTerminal, ListAllTerminals, ListTerminals,
    MAX_TERMINAL_INPUT, OpenTerminal, StartTerminal, Terminal, TerminalEvent, TerminalInput,
    TerminalOutput, TerminalState,
};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tauri::{State, ipc::Channel};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Keys and sizes waiting to go up one attached terminal's stream.
const INPUT_QUEUE: usize = 64;

type Service<'a> = State<'a, Arc<Beep>>;

/// One terminal as the window shows it.
#[derive(Debug, Serialize)]
pub struct TerminalRow {
    id: Uuid,
    /// `org/repo`, or empty for a terminal of no project.
    project: String,
    machine_id: String,
    machine_name: String,
    name: String,
    command: String,
    cwd: String,
    /// `running`, `exited`, `stopped`, `offline` or `gone`.
    state: &'static str,
    /// How it exited, when it did on its own.
    code: Option<i32>,
    /// In milliseconds since the Unix epoch.
    started_at: u64,
}

impl From<Terminal> for TerminalRow {
    fn from(t: Terminal) -> Self {
        let code = match t.state {
            TerminalState::Exited { code } => code,
            _ => None,
        };
        Self {
            id: t.id,
            project: t.project,
            machine_id: t.machine_id,
            machine_name: t.machine_name,
            name: t.name,
            command: t.command,
            cwd: t.cwd,
            state: t.state.into(),
            code,
            started_at: epoch_ms(t.started_at),
        }
    }
}

/// A terminal with the last of its output.
#[derive(Debug, Serialize)]
pub struct OutputView {
    terminal: TerminalRow,
    lines: Vec<String>,
    earlier: bool,
}

fn outdated(e: DesktopError) -> DesktopError {
    if unknown_op(&e) {
        DesktopError::BadRequest("The server needs an update before it keeps terminals.".into())
    } else {
        e
    }
}

/// `project`'s terminals; `None` from a server too old to keep them.
#[tauri::command]
pub async fn terminals_list(
    beep: Service<'_>,
    project: String,
) -> Result<Option<Vec<TerminalRow>>> {
    match beep.call(ListTerminals { project }).await {
        Ok(rows) => Ok(Some(rows.into_iter().map(Into::into).collect())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Every terminal on every machine; `None` from a server too old to list
/// them.
#[tauri::command]
pub async fn terminals_all(beep: Service<'_>) -> Result<Option<Vec<TerminalRow>>> {
    match beep.call(ListAllTerminals {}).await {
        Ok(rows) => Ok(Some(rows.into_iter().map(Into::into).collect())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The fields of a new terminal, as the form gives them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTerminal {
    project: String,
    machine_id: String,
    name: String,
    command: String,
    cwd: String,
}

#[tauri::command]
pub async fn terminal_start(beep: Service<'_>, terminal: NewTerminal) -> Result<TerminalRow> {
    let NewTerminal {
        project,
        machine_id,
        name,
        command,
        cwd,
    } = terminal;
    let started = beep
        .call(StartTerminal {
            project,
            machine_id,
            name,
            command,
            cwd,
        })
        .await
        .map_err(outdated)?;
    Ok(started.into())
}

/// The fields of a new interactive terminal, as the form gives them. An
/// empty `command` starts the shell.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewShell {
    project: String,
    machine_id: String,
    name: String,
    command: String,
    cwd: String,
    cols: u32,
    rows: u32,
}

/// Starts an interactive terminal; `None` from a server too old for them,
/// which still starts view-only ones with `terminal_start`.
#[tauri::command]
pub async fn terminal_open(beep: Service<'_>, terminal: NewShell) -> Result<Option<TerminalRow>> {
    let NewShell {
        project,
        machine_id,
        name,
        command,
        cwd,
        cols,
        rows,
    } = terminal;
    let opened = beep
        .call(OpenTerminal {
            project,
            machine_id,
            name,
            command,
            cwd,
            cols,
            rows,
        })
        .await;
    match opened {
        Ok(row) => Ok(Some(row.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn terminal_output(beep: Service<'_>, id: Uuid) -> Result<OutputView> {
    let TerminalOutput {
        terminal,
        lines,
        earlier,
    } = beep.call(GetTerminal { id }).await.map_err(outdated)?;
    Ok(OutputView {
        terminal: terminal.into(),
        lines,
        earlier,
    })
}

#[tauri::command]
pub async fn terminal_close(beep: Service<'_>, id: Uuid) -> Result<()> {
    beep.call(CloseTerminal { id }).await.map_err(outdated)
}

/// What an attached terminal tells the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TermEvent {
    /// The stream is open: keys typed from now on go to the terminal.
    Attached,
    /// Output from byte `at`, base64.
    Output { at: u64, data: String },
    /// It stopped, or its machine went offline: `state` as `TerminalRow`'s.
    Ended {
        state: &'static str,
        code: Option<i32>,
    },
    /// It takes no input; read it with `terminal_output`.
    ViewOnly,
}

impl From<TerminalEvent> for TermEvent {
    fn from(event: TerminalEvent) -> Self {
        match event {
            TerminalEvent::Output { at, data } => Self::Output {
                at,
                data: STANDARD.encode(data),
            },
            TerminalEvent::Ended { state } => Self::Ended {
                code: match state {
                    TerminalState::Exited { code } => code,
                    _ => None,
                },
                state: state.into(),
            },
            TerminalEvent::ViewOnly {} => Self::ViewOnly,
        }
    }
}

/// The terminals the windows follow, by terminal and view, each with
/// where its keys go. Two views of one terminal, such as a session's pane
/// and the Terminals page, keep a stream each; a view attaching again
/// replaces only its own older stream.
#[derive(Default)]
pub struct Attached(Mutex<Following>);

#[derive(Default)]
struct Following {
    next: u64,
    by_view: HashMap<Viewer, Follow>,
}

/// One view of one terminal. The window names its views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Viewer {
    terminal: Uuid,
    view: Uuid,
}

struct Follow {
    /// Which attach this is, so an older one ending leaves a newer alone.
    serial: u64,
    input: mpsc::Sender<TerminalInput>,
}

impl Attached {
    fn lock(&self) -> MutexGuard<'_, Following> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn follow(&self, viewer: Viewer, input: mpsc::Sender<TerminalInput>) -> u64 {
        let mut following = self.lock();
        following.next += 1;
        let serial = following.next;
        following.by_view.insert(viewer, Follow { serial, input });
        serial
    }

    fn unfollow(&self, viewer: Viewer, serial: u64) {
        let mut following = self.lock();
        if following
            .by_view
            .get(&viewer)
            .is_some_and(|f| f.serial == serial)
        {
            following.by_view.remove(&viewer);
        }
    }

    /// Drops `viewer`'s keys, which finishes its stream.
    fn detach(&self, viewer: Viewer) {
        self.lock().by_view.remove(&viewer);
    }

    fn input(&self, viewer: Viewer) -> Result<mpsc::Sender<TerminalInput>> {
        self.lock()
            .by_view
            .get(&viewer)
            .map(|follow| follow.input.clone())
            .ok_or_else(|| DesktopError::BadRequest("The terminal is not attached.".into()))
    }
}

/// Forgets an attach once its stream ends, however it ends.
struct Unfollow<'a> {
    attached: &'a Attached,
    viewer: Viewer,
    serial: u64,
}

impl Drop for Unfollow<'_> {
    fn drop(&mut self) {
        self.attached.unfollow(self.viewer, self.serial);
    }
}

/// Follows terminal `id` in `view` from byte `from` of its output into
/// `on_event`, sending up what `terminal_input` and `terminal_resize` give
/// for that view, until it ends or `terminal_detach`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn terminal_attach(
    beep: Service<'_>,
    attached: State<'_, Attached>,
    id: Uuid,
    view: Uuid,
    from: u64,
    cols: u32,
    rows: u32,
    on_event: Channel<TermEvent>,
) -> Result<()> {
    let (mut uplink, mut replies) = beep
        .duplex(AttachTerminal {
            id,
            from,
            cols,
            rows,
        })
        .await
        .map_err(outdated)?;
    let (input_tx, mut input) = mpsc::channel(INPUT_QUEUE);
    let viewer = Viewer { terminal: id, view };
    let serial = attached.follow(viewer, input_tx);
    let _unfollow = Unfollow {
        attached: &attached,
        viewer,
        serial,
    };
    on_event.send(TermEvent::Attached)?;
    loop {
        tokio::select! {
            reply = replies.next() => match reply.map_err(DesktopError::from).map_err(outdated)? {
                Some(event) => {
                    let last = !matches!(event, TerminalEvent::Output { .. });
                    on_event.send(event.into())?;
                    if last {
                        return Ok(());
                    }
                }
                None => return Ok(()),
            },
            typed = input.recv() => match typed {
                Some(up) => uplink.send(&up).await?,
                // Detached: finishing our side ends the stream.
                None => {
                    uplink.finish()?;
                    return Ok(());
                }
            },
        }
    }
}

/// Keys typed into attached terminal `id`, in pieces the server takes.
#[tauri::command]
pub async fn terminal_input(
    attached: State<'_, Attached>,
    id: Uuid,
    view: Uuid,
    data: String,
) -> Result<()> {
    let input = attached.input(Viewer { terminal: id, view })?;
    for piece in keys(data.as_bytes()) {
        input
            .send(piece)
            .await
            .map_err(|_| DesktopError::BadRequest("The terminal is no longer attached.".into()))?;
    }
    Ok(())
}

/// `data` as `Keys` of at most `MAX_TERMINAL_INPUT` bytes each.
fn keys(data: &[u8]) -> Vec<TerminalInput> {
    data.chunks(MAX_TERMINAL_INPUT)
        .map(|piece| TerminalInput::Keys {
            data: ByteBuf::from(piece.to_vec()),
        })
        .collect()
}

#[tauri::command]
pub async fn terminal_resize(
    attached: State<'_, Attached>,
    id: Uuid,
    view: Uuid,
    cols: u32,
    rows: u32,
) -> Result<()> {
    let input = attached.input(Viewer { terminal: id, view })?;
    input
        .send(TerminalInput::Resize { cols, rows })
        .await
        .map_err(|_| DesktopError::BadRequest("The terminal is no longer attached.".into()))
}

/// Stops following terminal `id` in `view`; it keeps running on its
/// machine, and other views of it stay attached.
#[tauri::command]
pub fn terminal_detach(attached: State<'_, Attached>, id: Uuid, view: Uuid) {
    attached.detach(Viewer { terminal: id, view });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn events_read_as_the_window_wants_them() {
        let output = TermEvent::from(TerminalEvent::Output {
            at: 7,
            data: ByteBuf::from(b"hi".to_vec()),
        });
        assert_eq!(
            output,
            TermEvent::Output {
                at: 7,
                data: "aGk=".into()
            }
        );
        let ended = TermEvent::from(TerminalEvent::Ended {
            state: TerminalState::Exited { code: Some(1) },
        });
        assert_eq!(
            ended,
            TermEvent::Ended {
                state: "exited",
                code: Some(1)
            }
        );
    }

    #[test]
    fn long_input_goes_in_pieces() {
        let pieces = keys(&vec![b'x'; MAX_TERMINAL_INPUT * 2 + 1]);
        assert_eq!(pieces.len(), 3);
    }

    #[test]
    fn an_older_attach_ending_leaves_a_newer_one() {
        let attached = Attached::default();
        let viewer = Viewer {
            terminal: Uuid::nil(),
            view: Uuid::nil(),
        };
        let (first, _) = mpsc::channel(1);
        let old = attached.follow(viewer, first);
        let (second, _) = mpsc::channel(1);
        attached.follow(viewer, second);
        attached.unfollow(viewer, old);
        assert!(attached.input(viewer).is_ok());
    }

    // Two views of one terminal used to share one slot, so each attach
    // finished the other's stream and they reattached in turn forever.
    #[test]
    fn two_views_of_a_terminal_keep_their_own_streams() {
        let attached = Attached::default();
        let terminal = Uuid::from_u128(1);
        let pane = Viewer {
            terminal,
            view: Uuid::from_u128(2),
        };
        let page = Viewer {
            terminal,
            view: Uuid::from_u128(3),
        };
        let (pane_tx, pane_rx) = mpsc::channel(1);
        attached.follow(pane, pane_tx);
        let (page_tx, _page_rx) = mpsc::channel(1);
        attached.follow(page, page_tx);
        assert!(
            !pane_rx.is_closed(),
            "the page attaching kept the pane's stream"
        );
        attached.detach(page);
        assert!(
            attached.input(pane).is_ok(),
            "the page detaching kept the pane"
        );
        assert!(!pane_rx.is_closed());
    }

    #[test]
    fn an_exited_terminal_keeps_its_code() {
        let row = TerminalRow::from(Terminal {
            id: Uuid::nil(),
            project: "acme/widget".into(),
            machine_id: "m1".into(),
            machine_name: "build-box".into(),
            name: "tests".into(),
            command: "just test".into(),
            cwd: "/srv/project".into(),
            state: TerminalState::Exited { code: Some(2) },
            started_at: UNIX_EPOCH,
        });
        assert_eq!(row.project, "acme/widget");
        assert_eq!(row.state, "exited");
        assert_eq!(row.code, Some(2));
    }
}
