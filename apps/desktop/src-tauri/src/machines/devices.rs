//! Android emulators and iOS simulators on the machines of the mesh
//! (`device/1`): listing, booting and shutting them down, and following one's
//! screen live while touching it, typing and pressing its buttons; and the
//! mobile platforms a session's checkout builds for. Machines whose node
//! does not serve `device/1` are left out.

use crate::{
    error::{DesktopError, Result},
    fleet::{Fleet, Machines},
    machines::{Registry, Unfollow},
};
use beep_proto::{
    ApiError,
    node::{
        LinkRx, LinkTx,
        device::{
            Device, DeviceId, Frame, Input, MAX_SIDE, MAX_TEXT, PROTOCOL, Platform, Reply, Request,
        },
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{
    Manager, State, WebviewWindow,
    ipc::{Channel, InvokeResponseBody},
};
use tokio::{
    sync::{mpsc, watch},
    time::timeout,
};
use uuid::Uuid;

/// How long a machine has to list its devices.
const LIST_LIMIT: Duration = Duration::from_secs(10);
/// How long a machine has to look through a checkout for its platforms.
const PLATFORMS_LIMIT: Duration = Duration::from_secs(20);
/// How long a machine has to start or stop a device.
const POWER_LIMIT: Duration = Duration::from_secs(60);
/// Machines asked at once.
const IN_FLIGHT: usize = 4;
/// Touches and keys waiting to go up one stream.
const UP_QUEUE: usize = 64;
/// The smallest frame side asked for.
const MIN_SIDE: u32 = 160;
/// How long a frame may stay unacknowledged before the next goes anyway, so
/// a message the window lost does not freeze the screen.
const DRAW_STALL: Duration = Duration::from_secs(1);

/// One machine's devices, or why it could not list them.
#[derive(Debug, Serialize)]
pub struct DeviceMachine {
    machine_id: String,
    machine_name: String,
    devices: Vec<Device>,
    error: Option<String>,
}

/// A machine that serves `device/1`.
#[derive(Debug, Serialize)]
pub struct DeviceHost {
    machine_id: String,
    machine_name: String,
}

/// The machines that are on and serve `device/1`, as last reported; no
/// machine is asked.
#[tauri::command]
pub fn device_hosts(fleet: Machines<'_>) -> Vec<DeviceHost> {
    let mut hosts: Vec<DeviceHost> = fleet
        .status()
        .offering(PROTOCOL)
        .into_iter()
        .map(|machine| DeviceHost {
            machine_id: machine.to_string(),
            machine_name: fleet.name_of(machine),
        })
        .collect();
    hosts.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    hosts
}

/// The devices of `machine_id`, or of every machine serving `device/1`,
/// asked at once.
#[tauri::command]
pub async fn device_list(
    fleet: Machines<'_>,
    machine_id: Option<String>,
) -> Result<Vec<DeviceMachine>> {
    let machines: Vec<EndpointId> = fleet
        .status()
        .offering(PROTOCOL)
        .into_iter()
        .filter(|machine| {
            machine_id
                .as_ref()
                .is_none_or(|wanted| *wanted == machine.to_string())
        })
        .collect();

    let fleet: &Fleet = &fleet;
    let mut listed: Vec<DeviceMachine> = stream::iter(machines)
        .map(|machine| async move {
            let (devices, error) = match list(fleet, machine).await {
                Ok(devices) => (devices, None),
                Err(e) => (Vec::new(), Some(e.to_string())),
            };
            DeviceMachine {
                machine_id: machine.to_string(),
                machine_name: fleet.name_of(machine),
                devices,
                error,
            }
        })
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;

    listed.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    Ok(listed)
}

async fn list(fleet: &Fleet, machine: EndpointId) -> Result<Vec<Device>> {
    match ask(fleet, machine, &Request::List {}, LIST_LIMIT).await? {
        Reply::Devices { devices } => Ok(devices),
        other => Err(answered(other)),
    }
}

/// The mobile platforms the app in folder `root` on `machine_id` builds
/// for. A node too old to tell, or one that may not read `root`, finds none;
/// a machine that cannot be reached is an error, so the window asks again
/// later.
#[tauri::command]
pub async fn device_platforms(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
) -> Result<Vec<Platform>> {
    let machine = member(&fleet, &machine_id)?;
    let client = fleet.client(machine).await?;
    let request = Request::Platforms { root };
    let (_tx, mut rx) = client.open_sending(PROTOCOL, &request).await?;
    let name = || fleet.name_of(machine);

    let heard = timeout(PLATFORMS_LIMIT, rx.recv::<Reply>())
        .await
        .map_err(|_| DesktopError::TimedOut(format!("{} did not answer in time.", name())))?;

    match heard {
        Ok(Some(Reply::Platforms { platforms })) => Ok(platforms),
        // A released node cannot read the request and closes the stream.
        Ok(None) | Err(_) => {
            log::debug!("{} does not tell a checkout's platforms", name());
            Ok(Vec::new())
        }
        Ok(Some(other)) => {
            log::debug!("{} found no platforms: {other:?}", name());
            Ok(Vec::new())
        }
    }
}

#[tauri::command]
pub async fn device_boot(fleet: Machines<'_>, machine_id: String, id: DeviceId) -> Result<()> {
    power(&fleet, &machine_id, Request::Boot { id }).await
}

#[tauri::command]
pub async fn device_shutdown(fleet: Machines<'_>, machine_id: String, id: DeviceId) -> Result<()> {
    power(&fleet, &machine_id, Request::Shutdown { id }).await
}

async fn power(fleet: &Fleet, machine_id: &str, request: Request) -> Result<()> {
    let machine = member(fleet, machine_id)?;

    match ask(fleet, machine, &request, POWER_LIMIT).await? {
        Reply::Done {} => Ok(()),
        other => Err(answered(other)),
    }
}

fn member(fleet: &Fleet, machine_id: &str) -> Result<EndpointId> {
    fleet
        .member(machine_id)
        .ok_or(DesktopError::Api(ApiError::NotFound {}))
}

/// Sends one request on a new stream and reads one reply within `limit`.
async fn ask(
    fleet: &Fleet,
    machine: EndpointId,
    request: &Request,
    limit: Duration,
) -> Result<Reply> {
    let client = fleet.client(machine).await?;
    let (_tx, mut rx) = client.open_sending(PROTOCOL, request).await?;
    let name = || fleet.name_of(machine);

    timeout(limit, rx.recv::<Reply>())
        .await
        .map_err(|_| DesktopError::TimedOut(format!("{} did not answer in time.", name())))??
        .ok_or_else(|| DesktopError::Lost(name()))
}

/// A reply that is not the one asked for: the node's reason when it gave one.
fn answered(reply: Reply) -> DesktopError {
    match reply {
        Reply::Failed { reason } | Reply::Ended { reason } => DesktopError::BadRequest(reason),
        other => DesktopError::Other(anyhow::anyhow!("unexpected device reply {other:?}")),
    }
}

/// What a followed device tells the window. A frame goes as raw bytes, its
/// width and height first as four little-endian bytes each, then the JPEG;
/// the rest goes as JSON tagged by `kind`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScreenEvent {
    Attached,
    #[serde(skip)]
    Frame(Frame),
    Ended {
        reason: String,
    },
}

impl ScreenEvent {
    /// The message the window's channel gets.
    pub fn body(self) -> Result<InvokeResponseBody> {
        if let Self::Frame(frame) = self {
            let mut bytes = Vec::with_capacity(8 + frame.jpeg.len());
            bytes.extend_from_slice(&frame.width.to_le_bytes());
            bytes.extend_from_slice(&frame.height.to_le_bytes());
            bytes.extend_from_slice(&frame.jpeg);
            return Ok(InvokeResponseBody::Raw(bytes));
        }

        let json = serde_json::to_string(&self).map_err(anyhow::Error::from)?;
        Ok(InvokeResponseBody::Json(json))
    }
}

/// The frames on their way to the window: one at a time, the next once the
/// window drew the last, and of those that came meanwhile only the newest.
#[derive(Debug)]
struct Screen {
    /// When the frame the window is drawing went; `None` when it is free.
    drawing: Option<Instant>,
    waiting: Option<Frame>,
}

impl Screen {
    fn new() -> Self {
        Self {
            drawing: None,
            waiting: None,
        }
    }

    /// Takes a frame from the node, and gives the one that goes now.
    fn arrived(&mut self, frame: Frame, now: Instant) -> Option<Frame> {
        let free = self
            .drawing
            .is_none_or(|since| now.duration_since(since) >= DRAW_STALL);

        if !free {
            self.waiting = Some(frame);
            return None;
        }

        self.waiting = None;
        self.drawing = Some(now);
        Some(frame)
    }

    /// The window drew the last frame: the newest that waits goes now.
    fn drawn(&mut self, now: Instant) -> Option<Frame> {
        let next = self.waiting.take();
        self.drawing = next.as_ref().map(|_| now);
        next
    }
}

/// Input and drawing acknowledgements for one attached view. Acknowledgements
/// coalesce independently so a full input queue cannot stall the screen.
#[derive(Clone)]
pub struct DeviceControls {
    input: mpsc::Sender<Input>,
    drawn: watch::Sender<()>,
}

/// The devices the windows follow, by view, each with where its input goes.
pub type Attached = Registry<Uuid, DeviceControls>;

/// Follows device `id` on `machine_id` in `view`: its frames, each with its
/// longer side at most `max_side`, go to `on_event` one at a time, and what
/// `device_input` gives for that view goes up, until the device's view ends
/// or `device_detach`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn device_attach(
    window: WebviewWindow,
    fleet: Machines<'_>,
    attached: State<'_, Attached>,
    machine_id: String,
    id: DeviceId,
    view: Uuid,
    max_side: u32,
    on_event: Channel<InvokeResponseBody>,
) -> Result<()> {
    let machine = member(&fleet, &machine_id)?;
    let client = fleet.client(machine).await?;
    let request = Request::Attach {
        id,
        max_side: max_side.clamp(MIN_SIDE, MAX_SIDE),
    };
    let (tx, rx) = client.open_sending(PROTOCOL, &request).await?;

    let (input, up) = mpsc::channel(UP_QUEUE);
    let (drawn, draws) = watch::channel(());
    let serial = attached.follow(view, window.label(), DeviceControls { input, drawn });
    let _unfollow = Unfollow {
        registry: &attached,
        key: view,
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

    on_event.send(ScreenEvent::Attached.body()?)?;
    let lost = format!("Lost the connection to {}.", fleet.name_of(machine));

    follow_screen(
        tx,
        rx,
        up,
        draws,
        |event| {
            on_event.send(event.body()?)?;
            Ok(())
        },
        &lost,
    )
    .await
}

async fn follow_screen(
    mut tx: LinkTx,
    mut rx: LinkRx,
    mut input: mpsc::Receiver<Input>,
    mut drawn: watch::Receiver<()>,
    mut send: impl FnMut(ScreenEvent) -> Result<()>,
    lost: &str,
) -> Result<()> {
    // Keep a pending input write alive while frames and acknowledgements run.
    // Dropping this future on detach also drops a blocked write and the stream.
    let inputs = async move {
        while let Some(input) = input.recv().await {
            if tx.send(&Request::Input { input }).await.is_err() {
                return false;
            }
        }

        true
    };
    tokio::pin!(inputs);
    let mut screen = Screen::new();
    let lost = || ScreenEvent::Ended {
        reason: lost.to_owned(),
    };

    loop {
        tokio::select! {
            heard = rx.recv::<Reply>() => match heard {
                Ok(Some(Reply::Frame(frame))) => {
                    if let Some(frame) = screen.arrived(frame, Instant::now()) {
                        send(ScreenEvent::Frame(frame))?;
                    }
                }
                Ok(Some(Reply::Ended { reason } | Reply::Failed { reason })) => {
                    send(ScreenEvent::Ended { reason })?;
                    return Ok(());
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    send(lost())?;
                    return Ok(());
                }
            },
            changed = drawn.changed() => {
                // Detached: dropping the stream ends the view on the node.
                if changed.is_err() {
                    return Ok(());
                }

                if let Some(frame) = screen.drawn(Instant::now()) {
                    send(ScreenEvent::Frame(frame))?;
                }
            },
            sent = &mut inputs => {
                if !sent {
                    send(lost())?;
                }

                return Ok(());
            }
        }
    }
}

/// Input to the device attached in `view`.
#[tauri::command]
pub async fn device_input(attached: State<'_, Attached>, view: Uuid, input: Input) -> Result<()> {
    let up = attached.get(&view, "device")?.input;

    for input in checked(input) {
        up.send(input)
            .await
            .map_err(|_| DesktopError::BadRequest("The device is no longer attached.".into()))?;
    }

    Ok(())
}

/// The window drew the last frame of the device attached in `view`.
#[tauri::command]
pub fn device_drawn(attached: State<'_, Attached>, view: Uuid) {
    if let Ok(up) = attached.get(&view, "device") {
        up.drawn.send_replace(());
    }
}

/// Stops following the device in `view`; it keeps running on its machine.
#[tauri::command]
pub fn device_detach(attached: State<'_, Attached>, view: Uuid) {
    attached.detach(&view);
}

/// `input` as the node takes it: a touch inside the screen, text in pieces
/// of at most `MAX_TEXT` bytes, and no empty text.
fn checked(input: Input) -> Vec<Input> {
    match input {
        Input::Touch { phase, x, y } if x.is_finite() && y.is_finite() => vec![Input::Touch {
            phase,
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        }],
        Input::Touch { .. } => Vec::new(),
        Input::Text { text } => pieces(&text, MAX_TEXT)
            .into_iter()
            .map(|text| Input::Text {
                text: text.to_owned(),
            })
            .collect(),
        other => vec![other],
    }
}

/// `text` cut on character boundaries into pieces of at most `max` bytes.
fn pieces(text: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;

    while !rest.is_empty() {
        let mut end = rest.len().min(max);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        // A character longer than `max` goes whole.
        if end == 0 {
            end = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        let (piece, tail) = rest.split_at(end);
        out.push(piece);
        rest = tail;
    }

    out
}

#[cfg(test)]
#[path = "../tests/machines_devices.rs"]
mod tests;
