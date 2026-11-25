//! Terminals for the window, of a project or on every machine: commands
//! kept running on the user's machines, their output, starting and closing them, and following
//! an interactive one live while typing into it.

use crate::{
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        terminals::{self as direct, Opened},
    },
    machines::{Registry, Unfollow, list::MachineLists},
};
use beep_mesh::machine::terminals::{Heard, heard};
use beep_proto::{
    MAX_TERMINAL_INPUT, OpenTerminal, Terminal, TerminalEvent, TerminalInput, TerminalOutput,
    TerminalState,
    node::{
        LinkTx,
        terminal::{Reply as PtyReply, Request as PtyRequest, Size},
    },
};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, PoisonError},
    time::Duration,
};
use tauri::{
    AppHandle, Manager, State, WebviewWindow,
    ipc::{Channel, InvokeResponseBody},
};
use tokio::{
    sync::mpsc,
    time::{Instant, timeout},
};
use uuid::Uuid;

/// Keys and sizes waiting to go up one attached terminal's stream.
const INPUT_QUEUE: usize = 256;
/// Bound on queuing keystrokes into a stalled terminal stream before telling the caller.
const INPUT_WAIT: Duration = Duration::from_millis(250);
/// Bound on writing one PTY frame over a degraded node link.
const WRITE_LIMIT: Duration = Duration::from_secs(10);
/// The most output one message to the window carries, and how long output
/// waits for more once a flood began: each message costs the webview a
/// script and a fetch, so a flood goes in few large ones.
const BATCH_BYTES: usize = 256 * 1024;
const BATCH_WAIT: Duration = Duration::from_millis(4);

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

/// Whether two listings of a terminal differ in what the window draws.
fn same_terminal(old: &Terminal, new: &Terminal) -> bool {
    old.id == new.id && old.state == new.state
}

/// Every terminal, at once: those on this Mac as they stand, the rest as
/// last held. The other machines of the mesh refresh in the background, and
/// the windows redraw when one changes.
async fn every(app: &AppHandle, fleet: &Arc<Fleet>, held: &MachineLists) -> Result<Vec<Terminal>> {
    let f = fleet.clone();
    let remote =
        held.terminals
            .refreshed(app, async move { direct::remote(&f).await }, same_terminal);
    let mut rows = direct::local(fleet).await?;
    rows.extend(direct::remote_now(fleet, &remote));
    Ok(rows)
}

/// Every terminal on every machine, each machine asked now: what the terminals view pushes.
pub(crate) async fn all_now(
    app: &AppHandle,
    fleet: &Arc<Fleet>,
    held: &MachineLists,
) -> Result<Vec<TerminalRow>> {
    let f = fleet.clone();
    let remote = held
        .terminals
        .read_now(app, async move { direct::remote(&f).await }, same_terminal)
        .await;
    let mut rows = direct::local(fleet).await?;
    rows.extend(direct::remote_now(fleet, &remote));

    Ok(rows.into_iter().map(Into::into).collect())
}

/// `project`'s terminals.
#[tauri::command]
pub async fn terminals_list(
    app: AppHandle,
    fleet: Machines<'_>,
    held: State<'_, MachineLists>,
    project: String,
) -> Result<Option<Vec<TerminalRow>>> {
    let rows = every(&app, &fleet, &held).await?;
    Ok(Some(
        rows.into_iter()
            .filter(|t| t.project == project)
            .map(Into::into)
            .collect(),
    ))
}

/// Every terminal on every machine.
#[tauri::command]
pub async fn terminals_all(
    app: AppHandle,
    fleet: Machines<'_>,
    held: State<'_, MachineLists>,
) -> Result<Option<Vec<TerminalRow>>> {
    let rows = every(&app, &fleet, &held).await?;
    Ok(Some(rows.into_iter().map(Into::into).collect()))
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

/// Starts an interactive terminal.
#[tauri::command]
pub async fn terminal_open(fleet: Machines<'_>, terminal: NewShell) -> Result<Option<TerminalRow>> {
    let NewShell {
        project,
        machine_id,
        name,
        command,
        cwd,
        cols,
        rows,
    } = terminal;
    let request = OpenTerminal {
        project,
        machine_id,
        name,
        command,
        cwd,
        cols,
        rows,
    };
    fleet.machine(&request.machine_id)?;
    Ok(Some(direct::open(&fleet, request).await?.into()))
}

#[tauri::command]
pub async fn terminal_output(fleet: Machines<'_>, id: Uuid) -> Result<OutputView> {
    let TerminalOutput {
        terminal,
        lines,
        earlier,
    } = direct::get(&fleet, id).await?;
    Ok(OutputView {
        terminal: terminal.into(),
        lines,
        earlier,
    })
}

#[tauri::command]
pub async fn terminal_close(
    app: AppHandle,
    fleet: Machines<'_>,
    held: State<'_, MachineLists>,
    id: Uuid,
) -> Result<()> {
    direct::close_with(Some(app.clone()), &fleet, id).await?;
    held.terminals.replace(Vec::new(), |t| t.id == id);
    crate::fleet::home_signal::poke(&app);
    Ok(())
}

/// What an attached terminal tells the window. Output goes as raw bytes,
/// the offset `at` first as eight little-endian bytes, so it is neither
/// base64 nor JSON; the rest goes as JSON tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TermEvent {
    /// The stream is open: keys typed from now on go to the terminal.
    Attached,
    /// Output from byte `at`.
    #[serde(skip)]
    Output { at: u64, data: Vec<u8> },
    /// It stopped, or its machine went offline: `state` as `TerminalRow`'s.
    Ended {
        state: &'static str,
        code: Option<i32>,
    },
    /// It takes no input; read it with `terminal_output`.
    ViewOnly,
}

impl TermEvent {
    /// The message the window's channel gets.
    pub fn body(self) -> Result<InvokeResponseBody> {
        if let Self::Output { at, data } = self {
            let mut bytes = Vec::with_capacity(8 + data.len());
            bytes.extend_from_slice(&at.to_le_bytes());
            bytes.extend_from_slice(&data);
            return Ok(InvokeResponseBody::Raw(bytes));
        }

        let json = serde_json::to_string(&self).map_err(anyhow::Error::from)?;
        Ok(InvokeResponseBody::Json(json))
    }
}

/// Output on its way to the window, gathered while it floods in. Output
/// after a quiet spell goes at once, so typing echoes without delay.
#[derive(Default)]
struct OutputBatch {
    at: u64,
    data: Vec<u8>,
    /// When the gathered output must go.
    due: Option<Instant>,
    /// When output last went.
    sent: Option<Instant>,
}

impl OutputBatch {
    /// Takes output from byte `at`, and gives what goes to the window now.
    fn add(&mut self, at: u64, data: &[u8], now: Instant) -> Vec<TermEvent> {
        let mut out = Vec::new();

        // Output that does not follow what is gathered goes apart.
        if !self.data.is_empty() && self.at + self.data.len() as u64 != at {
            out.extend(self.take(now));
        }

        if self.data.is_empty() {
            self.at = at;
        }
        self.data.extend_from_slice(data);
        let quiet = self
            .sent
            .is_none_or(|sent| now.duration_since(sent) >= BATCH_WAIT);

        if self.data.len() >= BATCH_BYTES || (quiet && self.due.is_none()) {
            out.extend(self.take(now));
        } else if self.due.is_none() {
            self.due = Some(now + BATCH_WAIT);
        }

        out
    }

    fn due(&self) -> Option<Instant> {
        self.due
    }

    /// The gathered output, which goes now.
    fn take(&mut self, now: Instant) -> Option<TermEvent> {
        self.due = None;

        if self.data.is_empty() {
            return None;
        }

        self.sent = Some(now);
        Some(TermEvent::Output {
            at: self.at,
            data: std::mem::take(&mut self.data),
        })
    }

    /// Sends `event` to the window: output gathered or as it comes, the rest
    /// after the output gathered before it.
    fn send(&mut self, on_event: &Channel<InvokeResponseBody>, event: TerminalEvent) -> Result<()> {
        let now = Instant::now();
        let events = match event {
            TerminalEvent::Output { at, data } => self.add(at, &data, now),
            other => self
                .take(now)
                .into_iter()
                .chain([TermEvent::from(other)])
                .collect(),
        };

        for event in events {
            on_event.send(event.body()?)?;
        }

        Ok(())
    }

    /// Sends the output gathered, when it is due.
    fn flush(&mut self, on_event: &Channel<InvokeResponseBody>) -> Result<()> {
        if let Some(event) = self.take(Instant::now()) {
            on_event.send(event.body()?)?;
        }

        Ok(())
    }
}

/// Waits until `batch`'s output is due; forever when none is gathered.
async fn batch_due(batch: &OutputBatch) {
    match batch.due() {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending().await,
    }
}

impl From<TerminalEvent> for TermEvent {
    fn from(event: TerminalEvent) -> Self {
        match event {
            TerminalEvent::Output { at, data } => Self::Output {
                at,
                data: data.into_vec(),
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
pub(crate) type Attached = Registry<Viewer, mpsc::Sender<TerminalInput>>;

/// One view of one terminal. The window names its views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Viewer {
    terminal: Uuid,
    view: Uuid,
}

/// Follows terminal `id` in `view` from byte `from` of its output into
/// `on_event`, sending up what `terminal_input` and `terminal_resize` give
/// for that view, until it ends or `terminal_detach`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn terminal_attach(
    window: WebviewWindow,
    fleet: Machines<'_>,
    attached: State<'_, Attached>,
    id: Uuid,
    view: Uuid,
    from: u64,
    cols: u32,
    rows: u32,
    on_event: Channel<InvokeResponseBody>,
) -> Result<()> {
    let (tx, mut rx) = match direct::attach(&fleet, id, from, cols, rows).await? {
        Opened::Live { tx, rx } => (tx, rx),
        Opened::Over(event) => {
            on_event.send(TermEvent::from(event).body()?)?;
            return Ok(());
        }
    };
    let (input_tx, input) = mpsc::channel(INPUT_QUEUE);
    let viewer = Viewer { terminal: id, view };
    let serial = attached.follow(viewer, window.label(), input_tx);
    let _unfollow = Unfollow {
        registry: &attached,
        key: viewer,
        serial,
    };

    // A window closed before the follow was recorded is not cleared.
    if window
        .app_handle()
        .get_webview_window(window.label())
        .is_none()
    {
        return Ok(());
    }

    on_event.send(TermEvent::Attached.body()?)?;
    let ended = |state| TerminalEvent::Ended { state };
    let mut batch = OutputBatch::default();
    let writes = send_pty_inputs(tx, input);
    tokio::pin!(writes);

    loop {
        tokio::select! {
            () = batch_due(&batch) => batch.flush(&on_event)?,
            heard_from_node = rx.recv::<PtyReply>() => match heard_from_node {
                Ok(Some(reply)) => match heard(reply) {
                    Heard::Show(event) => batch.send(&on_event, event)?,
                    Heard::End(state) => {
                        batch.send(&on_event, ended(state))?;
                        return Ok(());
                    }
                    Heard::Ignore => {}
                },
                Ok(None) | Err(_) => {
                    batch.send(&on_event, ended(TerminalState::Offline {}))?;
                    return Ok(());
                }
            },
            sent = &mut writes => {
                if !sent {
                    batch.send(&on_event, ended(TerminalState::Offline {}))?;
                }

                return Ok(());
            }
        }
    }
}

/// Converts a drained burst of queued terminal inputs into wire requests, coalescing
/// consecutive `Resize` frames so dragging a pane border over a slow link sends only the latest.
fn coalesce_pty_inputs(burst: impl IntoIterator<Item = TerminalInput>) -> Vec<PtyRequest> {
    let mut out = Vec::new();
    let mut pending_resize: Option<Size> = None;

    for item in burst {
        match item {
            TerminalInput::Resize { cols, rows } => {
                pending_resize = Some(Size::clamped(cols, rows));
            }
            TerminalInput::Keys { data } => {
                if let Some(size) = pending_resize.take() {
                    out.push(PtyRequest::Resize { size });
                }
                out.push(PtyRequest::Input { data });
            }
        }
    }

    if let Some(size) = pending_resize {
        out.push(PtyRequest::Resize { size });
    }

    out
}

/// Drains `input` onto `tx` without blocking output reading while `tx.send` awaits flow control.
/// Returns `true` on clean detach and `false` if the node link failed or timed out.
async fn send_pty_inputs(mut tx: LinkTx, mut input: mpsc::Receiver<TerminalInput>) -> bool {
    while let Some(first) = input.recv().await {
        let mut burst = vec![first];
        while let Ok(more) = input.try_recv() {
            burst.push(more);
        }

        for request in coalesce_pty_inputs(burst) {
            match timeout(WRITE_LIMIT, tx.send(&request)).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => return false,
            }
        }
    }

    true
}

/// Keys typed into attached terminal `id`, in pieces the machine takes.
#[tauri::command]
pub async fn terminal_input(
    attached: State<'_, Attached>,
    id: Uuid,
    view: Uuid,
    data: String,
) -> Result<()> {
    let input = attached.get(&Viewer { terminal: id, view }, "terminal")?;

    for piece in keys(data.as_bytes()) {
        match timeout(INPUT_WAIT, input.send(piece)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                return Err(DesktopError::BadRequest(
                    "The terminal is no longer attached.".into(),
                ));
            }
            Err(_) => {
                return Err(DesktopError::TimedOut(
                    "The terminal connection is congested.".into(),
                ));
            }
        }
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

static OVERFLOW_RESIZE: LazyLock<Mutex<HashMap<Viewer, (u32, u32)>>> =
    LazyLock::new(Mutex::default);

#[tauri::command]
pub async fn terminal_resize(
    attached: State<'_, Attached>,
    id: Uuid,
    view: Uuid,
    cols: u32,
    rows: u32,
) -> Result<()> {
    let viewer = Viewer { terminal: id, view };
    let input = attached.get(&viewer, "terminal")?;
    let mut overflow = OVERFLOW_RESIZE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);

    match input.try_send(TerminalInput::Resize { cols, rows }) {
        Ok(()) => {
            overflow.remove(&viewer);
            Ok(())
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            let start_drainer = overflow.insert(viewer, (cols, rows)).is_none();
            drop(overflow);

            if start_drainer {
                tauri::async_runtime::spawn(async move {
                    loop {
                        let Ok(permit) = input.reserve().await else {
                            OVERFLOW_RESIZE
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .remove(&viewer);
                            return;
                        };
                        let mut overflow = OVERFLOW_RESIZE
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        let Some((cols, rows)) = overflow.remove(&viewer) else {
                            return;
                        };
                        permit.send(TerminalInput::Resize { cols, rows });
                    }
                });
            }

            Ok(())
        }
        Err(mpsc::error::TrySendError::Closed(_)) => Err(DesktopError::BadRequest(
            "The terminal is no longer attached.".into(),
        )),
    }
}

/// Stops following terminal `id` in `view`; it keeps running on its
/// machine, and other views of it stay attached.
#[tauri::command]
pub fn terminal_detach(attached: State<'_, Attached>, id: Uuid, view: Uuid) {
    attached.detach(&Viewer { terminal: id, view });
}

#[cfg(test)]
#[path = "../tests/machines_terminals.rs"]
mod tests;
