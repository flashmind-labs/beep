//! Android emulators and iOS simulators on the machines of the mesh
//! (`device/1`): listing, booting and shutting them down, and following one's
//! screen live while touching it, typing and pressing its buttons; and the
//! mobile platforms a session's checkout builds for. Machines whose node
//! does not serve `device/1` are left out.

use crate::store::lock;
use crate::{
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        jobs::JobKind,
        node_actor::mark_device_power,
        projections::{DEVICE_LIST_LIMIT, Listing, Projected},
    },
    machines::{Registry, Unfollow},
};
use beep_proto::{
    ApiError,
    node::{
        LinkRx, LinkTx,
        device::{
            Device, DeviceId, DeviceState, Frame, Input, MAX_SIDE, MAX_TEXT, PROTOCOL, Platform,
            Reply, Request, TouchPhase,
        },
        device_video::{self, Rotation},
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use serde::Serialize;
use std::future::Future;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, State, WebviewWindow,
    ipc::{Channel, InvokeResponseBody},
};
use tokio::{
    sync::{Notify, mpsc, watch},
    time::timeout,
};
use uuid::Uuid;

/// Freshness window while a device on the machine is booting.
const BOOTING_LIST_FRESH: Duration = Duration::from_secs(1);
/// Freshness window while all devices on the machine are settled.
const IDLE_LIST_FRESH: Duration = Duration::from_secs(10);
/// Fast wait for power commands before handing off to the machine's background actor.
const POWER_FAST_WAIT: Duration = Duration::from_millis(300);
/// Bound on queuing discrete input into a stalled device view.
const INPUT_WAIT: Duration = Duration::from_millis(200);
/// Bound on writing one input request to a degraded device stream.
const WRITE_LIMIT: Duration = Duration::from_secs(10);
/// How long a machine has to look through a checkout for its platforms.
const PLATFORMS_LIMIT: Duration = Duration::from_secs(8);
/// Machines asked at once.
const IN_FLIGHT: usize = 4;
/// Touches and keys waiting to go up one stream.
const UP_QUEUE: usize = 64;
/// Queue slots kept free of `TouchPhase::Move` events so discrete presses and releases fit.
const RESERVED_DISCRETE: usize = 16;
/// The smallest frame side asked for.
const MIN_SIDE: u32 = 160;
/// How long a frame may stay unacknowledged before the next goes anyway, so
/// a message the window lost does not freeze the screen.
const DRAW_STALL: Duration = Duration::from_secs(1);
/// Distinguishes encoded bytes from the bounded JPEG width in legacy IPC.
const VIDEO_TAG: u32 = 0x31444956;

/// One machine's devices, or why it could not list them.
#[derive(Debug, Serialize)]
pub struct DeviceMachine {
    machine_id: String,
    machine_name: String,
    devices: Vec<Device>,
    error: Option<String>,
    /// The list has not arrived yet: `devices` is empty because nothing is
    /// known, not because the machine has none. It is pushed when it comes.
    loading: bool,
}

impl DeviceMachine {
    fn new(machine: EndpointId, name: String, outcome: Option<Result<Vec<Device>>>) -> Self {
        let (devices, error, loading) = match outcome {
            Some(Ok(devices)) => (devices, None, false),
            Some(Err(e)) => (Vec::new(), Some(e.to_string()), false),
            None => (Vec::new(), None, true),
        };

        Self {
            machine_id: machine.to_string(),
            machine_name: name,
            devices,
            error,
            loading,
        }
    }
}

/// The devices of `machine_id`, or of every machine serving `device/1`,
/// asked at once.
#[tauri::command]
pub async fn device_list(
    app: AppHandle,
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

    let fleet_arc = fleet.inner().clone();
    let mut listed: Vec<DeviceMachine> = stream::iter(machines)
        .map(|machine| {
            let app = app.clone();
            let fleet_arc = fleet_arc.clone();
            async move {
                let notify = move || {
                    let _ = app.emit("devices-changed", ());
                };
                let fleet_fetch = fleet_arc.clone();
                let outcome = read_devices(
                    fleet_arc.projections().devices.clone(),
                    machine,
                    notify,
                    move || async move { list(&fleet_fetch, machine).await },
                )
                .await;
                DeviceMachine::new(machine, fleet_arc.name_of(machine), outcome)
            }
        })
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;

    listed.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    Ok(listed)
}

/// What `machine` shows now: the held list, or none once the list took longer
/// than the projection's first wait. This machine's own node is capped the same
/// as any other, so a cold or hung node does not hold the window. The list
/// goes on being read for up to `DEVICE_LIST_LIMIT` and `notify` says when it
/// came, or that it failed: a machine with no list yet keeps the failure, and
/// one with a list keeps that list.
async fn read_devices<F, Fut>(
    devices: Arc<Projected<EndpointId, Listing>>,
    machine: EndpointId,
    notify: impl FnOnce() + Send + 'static,
    fetch: F,
) -> Option<Result<Vec<Device>>>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<Vec<Device>>> + Send + 'static,
{
    let fresh_for = |listing: &Listing| match listing {
        Listing::Devices(devices) if devices.iter().any(|d| d.state == DeviceState::Booting) => {
            BOOTING_LIST_FRESH
        }
        Listing::Devices(_) => IDLE_LIST_FRESH,
        Listing::Failed(_) => BOOTING_LIST_FRESH,
    };
    let held = devices.clone();
    let fetch = move || listing(held, machine, fetch);

    match devices
        .read_fast_with_fresh(machine, fresh_for, PartialEq::eq, notify, fetch)
        .await?
    {
        Ok(Listing::Devices(devices)) => Some(Ok(devices)),
        Ok(Listing::Failed(reason)) => Some(Err(DesktopError::BadRequest(reason))),
        Err(e) => Some(Err(e)),
    }
}

/// `machine`'s list from `fetch`, within `DEVICE_LIST_LIMIT` all told: reaching
/// the machine counts, so a node that never answers ends here, before the
/// projection's own limit. A failure becomes `Failed` unless the machine has
/// devices held, which it keeps.
async fn listing<F, Fut>(
    held: Arc<Projected<EndpointId, Listing>>,
    machine: EndpointId,
    fetch: F,
) -> Result<Listing>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<Device>>>,
{
    let read = timeout(DEVICE_LIST_LIMIT, fetch())
        .await
        .unwrap_or_else(|_| {
            Err(DesktopError::TimedOut(
                "The machine did not list its devices in time.".to_owned(),
            ))
        });

    match read {
        Ok(devices) => Ok(Listing::Devices(devices)),
        Err(e) if !matches!(held.peek(&machine), Some(Listing::Devices(_))) => {
            Ok(Listing::Failed(e.to_string()))
        }
        Err(e) => Err(e),
    }
}

async fn list(fleet: &Fleet, machine: EndpointId) -> Result<Vec<Device>> {
    match ask(fleet, machine, &Request::List {}, DEVICE_LIST_LIMIT).await? {
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
pub async fn device_boot(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    id: DeviceId,
) -> Result<()> {
    power(app, fleet.inner().clone(), &machine_id, id, true).await
}

#[tauri::command]
pub async fn device_shutdown(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    id: DeviceId,
) -> Result<()> {
    power(app, fleet.inner().clone(), &machine_id, id, false).await
}

async fn power(
    app: AppHandle,
    fleet: Arc<Fleet>,
    machine_id: &str,
    id: DeviceId,
    boot: bool,
) -> Result<()> {
    let machine = member(&fleet, machine_id)?;
    mark_device_power(&fleet, machine, &id, boot);
    let submitted = fleet
        .node_ops()
        .submit(
            Some(app),
            fleet.clone(),
            machine,
            JobKind::DevicePower { id, boot },
            POWER_FAST_WAIT,
        )
        .await;

    if submitted.is_err() {
        fleet.projections().devices.forget(&machine);
    }

    submitted.map(|_| ())
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
#[derive(PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScreenEvent {
    Attached,
    #[serde(skip)]
    Frame(Frame),
    Video {
        fps: u32,
    },
    #[serde(skip)]
    VideoData {
        data: Vec<u8>,
        rotation: Rotation,
    },
    Ended {
        reason: String,
    },
}

impl std::fmt::Debug for ScreenEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attached => f.write_str("Attached"),
            Self::Frame(frame) => f.debug_tuple("Frame").field(frame).finish(),
            Self::Video { fps } => f.debug_struct("Video").field("fps", fps).finish(),
            Self::VideoData { data, rotation } => f
                .debug_struct("VideoData")
                .field("bytes", &data.len())
                .field("rotation", rotation)
                .finish(),
            Self::Ended { reason } => f.debug_struct("Ended").field("reason", reason).finish(),
        }
    }
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

        if let Self::VideoData { data, rotation } = self {
            let mut bytes = Vec::with_capacity(8 + data.len());
            bytes.extend_from_slice(&VIDEO_TAG.to_le_bytes());
            bytes.extend_from_slice(&u32::from(rotation).to_le_bytes());
            bytes.extend_from_slice(&data);
            return Ok(InvokeResponseBody::Raw(bytes));
        }

        let json = serde_json::to_string(&self).map_err(anyhow::Error::from)?;
        Ok(InvokeResponseBody::Json(json))
    }
}

/// The frames on their way to the window: one at a time, the next once the
/// window drew the last, and of those that came meanwhile only the newest.
#[derive(Debug, Default)]
enum Screen {
    /// Nothing is being drawn.
    #[default]
    Free,
    /// The window is drawing the frame that went at the instant.
    Drawing(Instant),
    /// The same, with the newest frame that came since.
    Waiting(Instant, Frame),
}

impl Screen {
    /// Takes a frame from the node, and gives the one that goes now.
    fn arrived(&mut self, frame: Frame, now: Instant) -> Option<Frame> {
        if let Self::Drawing(since) | Self::Waiting(since, _) = *self
            && now.duration_since(since) < DRAW_STALL
        {
            *self = Self::Waiting(since, frame);
            return None;
        }

        *self = Self::Drawing(now);
        Some(frame)
    }

    /// The window drew the last frame: the newest that waits goes now.
    fn drawn(&mut self, now: Instant) -> Option<Frame> {
        match std::mem::replace(self, Self::Free) {
            Self::Waiting(_, frame) => {
                *self = Self::Drawing(now);
                Some(frame)
            }
            Self::Free | Self::Drawing(_) => None,
        }
    }
}

/// Input and drawing acknowledgements for one attached view. Acknowledgements
/// coalesce independently so a full input queue cannot stall the screen.
#[derive(Clone)]
pub struct DeviceControls {
    input: mpsc::Sender<Input>,
    touch_down: Arc<AtomicBool>,
    overflow_up: Arc<Mutex<Option<Input>>>,
    wake: Arc<Notify>,
    drawn: watch::Sender<()>,
}

impl DeviceControls {
    fn new(input: mpsc::Sender<Input>, drawn: watch::Sender<()>) -> Self {
        Self {
            input,
            touch_down: Arc::new(AtomicBool::new(false)),
            overflow_up: Arc::new(Mutex::new(None)),
            wake: Arc::new(Notify::new()),
            drawn,
        }
    }
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
    video: Option<bool>,
    on_event: Channel<InvokeResponseBody>,
) -> Result<()> {
    let machine = member(&fleet, &machine_id)?;
    let client = fleet.client(machine).await?;
    let max_side = max_side.clamp(MIN_SIDE, MAX_SIDE);
    let video = video.unwrap_or(false)
        && fleet
            .status()
            .offering(device_video::PROTOCOL)
            .contains(&machine);
    let (tx, rx) = if video {
        client
            .open_sending(
                device_video::PROTOCOL,
                &device_video::Request::Attach { id, max_side },
            )
            .await?
    } else {
        client
            .open_sending(PROTOCOL, &Request::Attach { id, max_side })
            .await?
    };

    let (input, up) = mpsc::channel(UP_QUEUE);
    let (drawn, draws) = watch::channel(());
    let controls = DeviceControls::new(input, drawn);
    let reader = InputReader::new(up, controls.overflow_up.clone(), controls.wake.clone());
    let serial = attached.follow(view, window.label(), controls);
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

    let send = |event: ScreenEvent| {
        on_event.send(event.body()?)?;
        Ok(())
    };
    if video {
        follow_video(tx, rx, reader, draws, send).await
    } else {
        follow_screen(tx, rx, reader, draws, send, &lost).await
    }
}

/// Reads device input, collapsing consecutive queued `TouchPhase::Move` events into the latest
/// coordinate once a write completes while draining any overflow `TouchPhase::Up` in causal order.
struct InputReader {
    rx: mpsc::Receiver<Input>,
    peeked: Option<Input>,
    overflow_up: Arc<Mutex<Option<Input>>>,
    wake: Arc<Notify>,
}

impl InputReader {
    fn new(
        rx: mpsc::Receiver<Input>,
        overflow_up: Arc<Mutex<Option<Input>>>,
        wake: Arc<Notify>,
    ) -> Self {
        Self {
            rx,
            peeked: None,
            overflow_up,
            wake,
        }
    }

    #[cfg(test)]
    fn from_channel(rx: mpsc::Receiver<Input>) -> Self {
        Self::new(rx, Arc::new(Mutex::new(None)), Arc::new(Notify::new()))
    }

    async fn recv_ordered(&mut self) -> Option<Input> {
        loop {
            match self.rx.try_recv() {
                Ok(item) => return Some(item),
                Err(mpsc::error::TryRecvError::Empty) => {
                    if let Some(release) = lock(&self.overflow_up).take() {
                        return Some(release);
                    }

                    tokio::select! {
                        item = self.rx.recv() => {
                            return item.or_else(|| {
                                lock(&self.overflow_up)
                                    .take()
                            });
                        }
                        () = self.wake.notified() => {}
                    }
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return lock(&self.overflow_up).take();
                }
            }
        }
    }

    async fn next(&mut self) -> Option<Input> {
        let mut current = match self.peeked.take() {
            Some(item) => item,
            None => self.recv_ordered().await?,
        };

        if matches!(
            current,
            Input::Touch {
                phase: TouchPhase::Move,
                ..
            }
        ) {
            while let Ok(next) = self.rx.try_recv() {
                if matches!(
                    next,
                    Input::Touch {
                        phase: TouchPhase::Move,
                        ..
                    }
                ) {
                    current = next;
                } else {
                    self.peeked = Some(next);
                    break;
                }
            }
        }

        Some(current)
    }
}

/// Every encoded chunk is acknowledged before reading the next. This bounds
/// IPC memory and preserves byte order without dropping reference frames.
async fn follow_video(
    mut tx: LinkTx,
    mut rx: LinkRx,
    mut reader: InputReader,
    mut drawn: watch::Receiver<()>,
    mut send: impl FnMut(ScreenEvent) -> Result<()>,
) -> Result<()> {
    let inputs = async move {
        while let Some(input) = reader.next().await {
            match timeout(
                WRITE_LIMIT,
                tx.send(&device_video::Request::Input { input }),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(err)) => return Err(err.into()),
                Err(_) => {
                    return Err(DesktopError::TimedOut(
                        "The device connection did not accept input in time.".into(),
                    ));
                }
            }
        }
        Ok(())
    };
    let frames = async {
        loop {
            let reply = rx
                .recv::<device_video::Reply>()
                .await?
                .ok_or_else(|| DesktopError::BadRequest("The video stream ended.".into()))?;
            match reply {
                device_video::Reply::Started { fps } => send(ScreenEvent::Video { fps })?,
                device_video::Reply::Data { data, rotation } => {
                    if data.is_empty() || data.len() > device_video::MAX_CHUNK {
                        return Err(DesktopError::BadRequest("Invalid video chunk.".into()));
                    }
                    send(ScreenEvent::VideoData { data, rotation })?;
                }
                device_video::Reply::Unsupported { reason }
                | device_video::Reply::Ended { reason } => {
                    return Err(DesktopError::BadRequest(reason));
                }
            }
            // A lost IPC message must not strand the byte stream. The page
            // opens a fresh JPEG view if ordered video delivery cannot continue.
            match timeout(DRAW_STALL, drawn.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => return Ok(()),
                Err(_) => {
                    return Err(DesktopError::BadRequest(
                        "The video viewer stopped receiving data.".into(),
                    ));
                }
            }
        }
    };
    tokio::select! {
        result = inputs => result,
        result = frames => result,
    }
}

async fn follow_screen(
    mut tx: LinkTx,
    mut rx: LinkRx,
    mut reader: InputReader,
    mut drawn: watch::Receiver<()>,
    mut send: impl FnMut(ScreenEvent) -> Result<()>,
    lost: &str,
) -> Result<()> {
    // Keep a pending input write alive while frames and acknowledgements run.
    // Dropping this future on detach also drops a blocked write and the stream.
    let inputs = async move {
        while let Some(input) = reader.next().await {
            match timeout(WRITE_LIMIT, tx.send(&Request::Input { input })).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => return false,
            }
        }

        true
    };
    tokio::pin!(inputs);
    let mut screen = Screen::default();
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
    let controls = attached.get(&view, "device")?;
    send_device_input(&controls, input).await
}

async fn send_device_input(controls: &DeviceControls, input: Input) -> Result<()> {
    let up = &controls.input;

    for input in checked(input) {
        if matches!(
            input,
            Input::Touch {
                phase: TouchPhase::Move,
                ..
            }
        ) {
            if up.is_closed() {
                return Err(DesktopError::BadRequest(
                    "The device is no longer attached.".into(),
                ));
            }
            if controls.touch_down.load(Ordering::Acquire) && up.capacity() > RESERVED_DISCRETE {
                let _ = up.try_send(input);
            }
            continue;
        }

        if matches!(
            input,
            Input::Touch {
                phase: TouchPhase::Up,
                ..
            }
        ) {
            if !controls.touch_down.swap(false, Ordering::AcqRel) {
                continue;
            }
            match up.try_send(input) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(release)) => {
                    *lock(&controls.overflow_up) = Some(release);
                    controls.wake.notify_one();
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    return Err(DesktopError::BadRequest(
                        "The device is no longer attached.".into(),
                    ));
                }
            }
            continue;
        }

        // Flush any earlier overflow release into a freed slot before sending newer discrete input.
        let pending_up = lock(&controls.overflow_up).take();
        if let Some(release) = pending_up
            && let Err(mpsc::error::TrySendError::Full(still_full)) = up.try_send(release)
        {
            *lock(&controls.overflow_up) = Some(still_full);
            return Err(DesktopError::TimedOut(
                "The device connection is congested.".into(),
            ));
        }

        let is_down = matches!(
            input,
            Input::Touch {
                phase: TouchPhase::Down,
                ..
            }
        );

        match timeout(INPUT_WAIT, up.send(input)).await {
            Ok(Ok(())) => {
                if is_down {
                    controls.touch_down.store(true, Ordering::Release);
                }
            }
            Ok(Err(_)) => {
                return Err(DesktopError::BadRequest(
                    "The device is no longer attached.".into(),
                ));
            }
            Err(_) => {
                if is_down {
                    controls.touch_down.store(false, Ordering::Release);
                }
                return Err(DesktopError::TimedOut(
                    "The device connection is congested.".into(),
                ));
            }
        }
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
