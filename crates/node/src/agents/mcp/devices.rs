//! The `device` tool: the Android emulators and iOS simulators of every
//! machine of the mesh, listed, booted, shut down, shown in the user's
//! Device tab, and seen and driven by the agent. Each call goes over
//! `device/1`, to this node's own service or to another machine's, which
//! serves it only while it is lent to other devices. A device the agent
//! drives keeps one attached stream open until it idles.

use super::{
    machines::{self, Machine},
    server::Called,
};
use crate::{agents::calls::command_name, link::Offers, run::device};
use anyhow::{Context, Result, bail};
use beep_mesh::Mesh;
use beep_proto::SessionId;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::SessionDevice,
    device::{
        Button, Device, DeviceId, DeviceState, Element, Frame, Input, Key, MAX_LOG_LINES, MAX_TEXT,
        Platform, Reply, Request, TouchPhase,
    },
};
use iroh::EndpointId;
use serde::Deserialize;
use serde_json::Value;
use std::{fmt::Write, sync::Mutex, time::Duration};
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, sleep, sleep_until, timeout},
};
use tokio_util::task::AbortOnDropHandle;

/// Why a call to an attached device fails once its view is gone.
const VIEW_STOPPED: &str = "the device's view stopped";
/// How long a machine has to list its devices.
const LIST_LIMIT: Duration = Duration::from_secs(15);
/// How long a machine has to start or stop a device.
const POWER_LIMIT: Duration = Duration::from_secs(60);
/// How long reading what a screen shows may take.
const UI_LIMIT: Duration = Duration::from_secs(30);
/// How long an install may take, a large app over USB included.
const INSTALL_LIMIT: Duration = Duration::from_secs(300);
/// The most elements a `ui` call lists.
const MAX_ELEMENTS: usize = 300;
/// How long reading a device's logs may take.
const LOGS_LIMIT: Duration = Duration::from_secs(30);
/// The log lines a `logs` call gets when it does not say.
const LOG_LINES: u32 = 200;
/// How long the first frame of a device may take.
const SHOT_LIMIT: Duration = Duration::from_secs(15);
/// The longer side of a screenshot, in pixels.
const SHOT_SIDE: u32 = 1280;
/// How long input settles before the screenshot that shows its result.
const SETTLE: Duration = Duration::from_millis(500);
/// How long an attached stream stays open with no call.
const IDLE: Duration = Duration::from_secs(30);
/// The most devices kept attached at once.
const MAX_ATTACHED: usize = 4;
/// A swipe's length when the call gives none, and its longest.
const SWIPE_DEFAULT: Duration = Duration::from_millis(300);
const SWIPE_MAX: Duration = Duration::from_secs(5);
/// The moves between a swipe's touch down and up.
const SWIPE_MOVES: u32 = 8;

/// A `device` call, read from its arguments.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    List {},
    Boot(Target),
    Shutdown(Target),
    Show(Target),
    Screenshot(Target),
    Tap {
        #[serde(flatten)]
        at: Target,
        x: f64,
        y: f64,
    },
    Swipe {
        #[serde(flatten)]
        at: Target,
        x: f64,
        y: f64,
        x2: f64,
        y2: f64,
        duration_ms: Option<u64>,
    },
    Type {
        #[serde(flatten)]
        at: Target,
        text: String,
    },
    Press {
        #[serde(flatten)]
        at: Target,
        key: Press,
    },
    Logs {
        #[serde(flatten)]
        at: Target,
        lines: Option<u32>,
        filter: Option<String>,
    },
    Ui(Target),
    /// `text` is the APK or `.app` path on the device's machine.
    Install {
        #[serde(flatten)]
        at: Target,
        text: String,
    },
    /// `text` is a package, bundle id or URL.
    Launch {
        #[serde(flatten)]
        at: Target,
        text: String,
    },
    /// `text` is an Android package.
    Reset {
        #[serde(flatten)]
        at: Target,
        text: String,
    },
}

/// The device a call names: the machine by its name in the list, the
/// device by its name or id.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Target {
    machine: String,
    device: String,
}

/// A key or hardware button, as the agent names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Press {
    Return,
    Delete,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    Back,
    AppSwitch,
    Power,
    VolumeUp,
    VolumeDown,
    Rotate,
}

impl From<Press> for Input {
    fn from(press: Press) -> Self {
        let key = |key| Self::Key { key };
        let button = |button| Self::Button { button };

        match press {
            Press::Return => key(Key::Return),
            Press::Delete => key(Key::Delete),
            Press::Tab => key(Key::Tab),
            Press::Escape => key(Key::Escape),
            Press::Left => key(Key::Left),
            Press::Right => key(Key::Right),
            Press::Up => key(Key::Up),
            Press::Down => key(Key::Down),
            Press::Home => button(Button::Home),
            Press::Back => button(Button::Back),
            Press::AppSwitch => button(Button::AppSwitch),
            Press::Power => button(Button::Power),
            Press::VolumeUp => button(Button::VolumeUp),
            Press::VolumeDown => button(Button::VolumeDown),
            Press::Rotate => button(Button::Rotate),
        }
    }
}

impl TryFrom<Value> for Action {
    type Error = String;

    fn try_from(args: Value) -> Result<Self, String> {
        serde_json::from_value(args).map_err(|e| format!("Bad arguments: {e}."))
    }
}

/// Runs one `device` call for `session`, which keeps the device the call used.
pub(super) async fn call(
    offers: &Offers,
    mesh: &Mesh,
    screens: &Screens,
    session: &SessionId,
    args: Value,
) -> Called {
    let action = match Action::try_from(args) {
        Ok(action) => action,
        Err(e) => return Called::failed(e),
    };
    let reach = Reach { offers, mesh };

    let (called, used) = match run(&reach, screens, action).await {
        Ok(ran) => ran,
        Err(e) => return Called::failed(format!("{e:#}")),
    };

    if let Some(device) = used
        && let Err(e) = offers.agents.set_device(session, Some(device)).await
    {
        log::debug!("the session did not keep its device: {e:#}");
    }

    called
}

/// What a call answers, and the device it used, if it used one that runs.
type Ran = (Called, Option<SessionDevice>);

struct Reach<'a> {
    offers: &'a Offers,
    mesh: &'a Mesh,
}

async fn run(reach: &Reach<'_>, screens: &Screens, action: Action) -> Result<Ran> {
    match action {
        Action::List {} => Ok((Called::ok(reach.list().await), None)),
        Action::Boot(at) => {
            let (machine, device) = reach.find(&at).await?;
            let used = used(&machine, &device);
            let shown = "It shows in the user's Device tab.";

            if device.state != DeviceState::Off {
                let text = format!("{} on {} is already on. {shown}", device.name, machine.name);
                return Ok((Called::ok(text), Some(used)));
            }

            reach
                .done(&machine, &Request::Boot { id: device.id }, POWER_LIMIT)
                .await?;
            let text = format!(
                "Booting {} on {}; it can take a minute to start. {shown}",
                device.name, machine.name
            );

            Ok((Called::ok(text), Some(used)))
        }
        Action::Shutdown(at) => {
            let (machine, device) = reach.find(&at).await?;

            if device.phone {
                bail!(
                    "{} on {} is a phone: only the user turns it off.",
                    device.name,
                    machine.name
                );
            }

            screens.close(machine.id, &device.id.0, &at.device);
            let req = Request::Shutdown { id: device.id };
            reach.done(&machine, &req, POWER_LIMIT).await?;

            Ok((
                Called::ok(format!("Shut down {} on {}.", device.name, machine.name)),
                None,
            ))
        }
        Action::Logs { at, lines, filter } => {
            let (machine, device) = reach.find(&at).await?;
            let req = Request::Logs {
                id: device.id.clone(),
                lines: lines.unwrap_or(LOG_LINES).min(MAX_LOG_LINES),
                filter: filter.unwrap_or_default(),
            };
            let text = match reach.ask(&machine, &req, LOGS_LIMIT).await? {
                Reply::Logs { text } if text.is_empty() => "No log lines match.".to_owned(),
                Reply::Logs { text } => text,
                Reply::Failed { reason } => bail!("{reason}"),
                other => bail!("{} answered {other:?}", machine.name),
            };

            Ok((Called::ok(text), Some(used(&machine, &device))))
        }
        Action::Ui(at) => {
            let (viewer, used) = screens.viewer(reach, &at).await?;
            let shot = viewer.shot().await?;
            let (machine, device) = reach.find(&at).await?;
            let elements = match reach
                .ask(&machine, &Request::Ui { id: device.id }, UI_LIMIT)
                .await?
            {
                Reply::Ui { elements } => elements,
                Reply::Failed { reason } => bail!("{reason}"),
                other => bail!("{} answered {other:?}", machine.name),
            };

            Ok((
                Called::ok(placed(&elements, shot.width, shot.height)),
                Some(used),
            ))
        }
        Action::Install { at, text } => {
            reach
                .app(&at, INSTALL_LIMIT, "Installed", &text, |id, path| {
                    Request::Install { id, path }
                })
                .await
        }
        Action::Launch { at, text } => {
            reach
                .app(&at, LIST_LIMIT, "Opened", &text, |id, target| {
                    Request::Launch { id, target }
                })
                .await
        }
        Action::Reset { at, text } => {
            reach
                .app(&at, LIST_LIMIT, "Cleared", &text, |id, app| {
                    Request::Reset { id, app }
                })
                .await
        }
        Action::Show(at) => {
            let (machine, device) = reach.find(&at).await?;
            ensure_on(&machine, &device)?;
            let text = format!(
                "{} on {} shows in the user's Device tab.",
                device.name, machine.name
            );

            Ok((Called::ok(text), Some(used(&machine, &device))))
        }
        Action::Screenshot(at) => {
            let (viewer, used) = screens.viewer(reach, &at).await?;
            Ok((shown(&at, viewer.shot().await?)?, Some(used)))
        }
        Action::Tap { at, x, y } => {
            let (viewer, used) = screens.viewer(reach, &at).await?;
            let (x, y) = fraction(x, y, &viewer.shot().await?)?;
            let inputs = vec![
                (touch(TouchPhase::Down, x, y), Duration::from_millis(50)),
                (touch(TouchPhase::Up, x, y), Duration::ZERO),
            ];
            Ok((drive(&viewer, &at, inputs).await?, Some(used)))
        }
        Action::Swipe {
            at,
            x,
            y,
            x2,
            y2,
            duration_ms,
        } => {
            let (viewer, used) = screens.viewer(reach, &at).await?;
            let last = viewer.shot().await?;
            let length = duration_ms
                .map_or(SWIPE_DEFAULT, Duration::from_millis)
                .min(SWIPE_MAX);
            let inputs = swipe(fraction(x, y, &last)?, fraction(x2, y2, &last)?, length);
            Ok((drive(&viewer, &at, inputs).await?, Some(used)))
        }
        Action::Type { at, text } => {
            if text.is_empty() || text.len() > MAX_TEXT {
                bail!("Give text of 1 to {MAX_TEXT} bytes.");
            }

            send_input(screens, reach, &at, Input::Text { text }).await
        }
        Action::Press { at, key } => send_input(screens, reach, &at, Input::from(key)).await,
    }
}

async fn send_input(
    screens: &Screens,
    reach: &Reach<'_>,
    at: &Target,
    input: Input,
) -> Result<Ran> {
    let (viewer, used) = screens.viewer(reach, at).await?;
    Ok((
        drive(&viewer, at, vec![(input, Duration::ZERO)]).await?,
        Some(used),
    ))
}

fn ensure_on(machine: &Machine, device: &Device) -> Result<()> {
    if device.state == DeviceState::Off {
        bail!(
            "{} on {} is off: device_boot it.",
            device.name,
            machine.name
        );
    }

    Ok(())
}

/// `device` on `machine`, as a session keeps it.
fn used(machine: &Machine, device: &Device) -> SessionDevice {
    SessionDevice {
        machine: machine.id,
        device: device.id.clone(),
    }
}

/// Sends `inputs`, each followed by its pause, then gives the screen once
/// it settled.
async fn drive(viewer: &Viewer, at: &Target, inputs: Vec<(Input, Duration)>) -> Result<Called> {
    viewer.input(inputs).await?;
    sleep(SETTLE).await;
    shown(at, viewer.shot().await?)
}

/// A line per element, with where to tap it in a `width` by `height`
/// screenshot.
fn placed(elements: &[Element], width: u32, height: u32) -> String {
    if elements.is_empty() {
        return "Nothing on the screen has a label or an id.".to_owned();
    }

    let (width, height) = (width as f32, height as f32);

    elements
        .iter()
        .take(MAX_ELEMENTS)
        .map(|element| {
            let x = ((element.x + element.width / 2.0) * width).round();
            let y = ((element.y + element.height / 2.0) * height).round();
            let id = if element.id.is_empty() {
                String::new()
            } else {
                format!(" #{}", element.id)
            };
            format!("- {:?} {}{id} at {x}, {y}", element.label, element.kind)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn shown(at: &Target, frame: Frame) -> Result<Called> {
    let mut called = Called::ok(format!(
        "{} on {}: {} x {} pixels.",
        at.device,
        command_name(&at.machine),
        frame.width,
        frame.height
    ));
    called.image = Some(frame);
    Ok(called)
}

fn touch(phase: TouchPhase, x: f32, y: f32) -> Input {
    Input::Touch { phase, x, y }
}

/// Pixel `x`, `y` of screenshot `frame` as fractions of its width and
/// height, as `device/1` takes touches.
fn fraction(x: f64, y: f64, frame: &Frame) -> Result<(f32, f32)> {
    let (width, height) = (f64::from(frame.width), f64::from(frame.height));
    let on = |v: f64, side: f64| v.is_finite() && (0.0..=side).contains(&v);

    if !on(x, width) || !on(y, height) || width <= 0.0 || height <= 0.0 {
        bail!(
            "{x}, {y} is off the screen: the last screenshot is {} x {} pixels.",
            frame.width,
            frame.height
        );
    }

    // A fraction of a screen fits an f32.
    #[allow(clippy::cast_possible_truncation)]
    Ok(((x / width) as f32, (y / height) as f32))
}

/// A swipe from `from` to `to` taking `length`: down, evenly spaced moves,
/// up, each with the pause after it.
fn swipe(from: (f32, f32), to: (f32, f32), length: Duration) -> Vec<(Input, Duration)> {
    let steps = SWIPE_MOVES + 1;
    let gap = length / steps;
    let point = |step: u32| {
        // At most `steps`, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let t = step as f32 / steps as f32;
        (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
    };
    let mut inputs = vec![(touch(TouchPhase::Down, from.0, from.1), gap)];

    for step in 1..steps {
        let (x, y) = point(step);
        inputs.push((touch(TouchPhase::Move, x, y), gap));
    }

    inputs.push((touch(TouchPhase::Up, to.0, to.1), Duration::ZERO));
    inputs
}

// ----- Machines and their devices -----

impl Reach<'_> {
    /// Every device on every machine that is online, each machine asked at once.
    async fn list(&self) -> String {
        let me = self.mesh.me();
        let online: Vec<Machine> = self
            .mesh
            .code_hosts()
            .into_iter()
            .filter(|status| status.online || status.device.id == me)
            .map(Machine::from)
            .collect();
        let listed = futures::future::join_all(
            online
                .iter()
                .map(|m| async move { (m, self.devices_on(m).await) }),
        )
        .await;
        let mut text = String::new();

        for (machine, devices) in listed {
            let here = if machine.id == me {
                " (this machine)"
            } else {
                ""
            };
            // Writing to a string cannot fail.
            let _ = match devices {
                Ok(devices) if devices.is_empty() => {
                    writeln!(text, "- {}{here}: no emulators or simulators", machine.name)
                }
                Ok(devices) => writeln!(text, "- {}{here}:\n{}", machine.name, lines(&devices)),
                Err(e) => writeln!(text, "- {}{here}: not listed: {e:#}", machine.name),
            };
        }

        if text.is_empty() {
            return "No machine is online.".to_owned();
        }

        text.trim_end().to_owned()
    }

    /// The machine and device `at` names.
    async fn find(&self, at: &Target) -> Result<(Machine, Device)> {
        let machine = machine(self.mesh, &at.machine)?;
        let devices = self
            .devices_on(&machine)
            .await
            .with_context(|| format!("{} did not list its devices", machine.name))?;
        let device =
            pick(&devices, &at.device).map_err(|e| anyhow::anyhow!("{e} on {}", machine.name))?;

        Ok((machine, device))
    }

    async fn devices_on(&self, machine: &Machine) -> Result<Vec<Device>> {
        match self.ask(machine, &Request::List {}, LIST_LIMIT).await? {
            Reply::Devices { devices } => Ok(devices),
            Reply::Failed { reason } => bail!(reason),
            other => bail!("unexpected reply {other:?}"),
        }
    }

    async fn app(
        &self,
        at: &Target,
        limit: Duration,
        verb: &str,
        text: &str,
        make: impl FnOnce(DeviceId, String) -> Request,
    ) -> Result<Ran> {
        let (machine, device) = self.find(at).await?;
        let req = make(device.id.clone(), text.to_owned());
        self.done(&machine, &req, limit).await?;
        let said = format!("{verb} {text} on {}.", device.name);

        Ok((Called::ok(said), Some(used(&machine, &device))))
    }

    async fn done(&self, machine: &Machine, request: &Request, limit: Duration) -> Result<()> {
        match self.ask(machine, request, limit).await? {
            Reply::Done {} => Ok(()),
            Reply::Failed { reason } => bail!(reason),
            other => bail!("unexpected reply {other:?}"),
        }
    }

    /// Sends one request to `machine`'s `device/1` and reads its reply.
    async fn ask(&self, machine: &Machine, request: &Request, limit: Duration) -> Result<Reply> {
        let mut link = self.open(machine).await?;
        link.tx.send(request).await?;

        timeout(limit, link.rx.recv())
            .await
            .with_context(|| format!("{} did not answer in time", machine.name))??
            .with_context(|| format!("{} closed the call", machine.name))
    }

    /// Opens `device/1` on `machine`: this node's own service in process, or
    /// another machine's over the mesh, which refuses while it is not lent.
    async fn open(&self, machine: &Machine) -> Result<Link> {
        if machine.id != self.mesh.me() {
            let (tx, rx) = crate::peer_stream::handshake(self.mesh, machine.id, device::PROTOCOL)
                .await
                .with_context(|| format!("could not reach {}", machine.name))?;
            return Ok(Link {
                tx,
                rx,
                _here: None,
            });
        }

        let link = beep_proto::node::local_link();
        let (node_tx, node_rx) = link.node;
        let agents = self.offers.agents.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = device::serve(node_tx, node_rx, agents).await {
                log::debug!("an agent's device stream ended: {e:#}");
            }
        });
        let (tx, rx) = link.caller;

        Ok(Link {
            tx,
            rx,
            _here: Some(AbortOnDropHandle::new(task)),
        })
    }
}

/// One line per device: its name, id, OS and state, and whether it is a
/// phone.
fn lines(devices: &[Device]) -> String {
    devices
        .iter()
        .map(|device| {
            let os = device.os.as_deref().unwrap_or(match device.platform {
                Platform::Android => "Android",
                Platform::Ios => "iOS",
            });
            let state = match device.state {
                DeviceState::Off => "off",
                DeviceState::Booting => "booting",
                DeviceState::On => "on",
            };
            let phone = if device.phone { ", phone" } else { "" };
            format!(
                "  - {} (id {}): {os}, {state}{phone}",
                device.name, device.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `device` takes taps, typing and keys: an iPhone does not, as
/// `devicectl` has no input.
fn takes_input(device: &Device) -> bool {
    !(device.phone && device.platform == Platform::Ios)
}

fn machine(mesh: &Mesh, name: &str) -> Result<Machine> {
    machines::find_or_id_named(mesh, name).map_err(anyhow::Error::msg)
}

/// The device `named` by id, else by name; a name several devices share
/// needs the id.
fn pick(devices: &[Device], named: &str) -> Result<Device, String> {
    let named = named.trim();

    if let Some(device) = devices.iter().find(|d| d.id.0 == named) {
        return Ok(device.clone());
    }

    let by_name: Vec<&Device> = devices
        .iter()
        .filter(|d| d.name.eq_ignore_ascii_case(named))
        .collect();

    match by_name.as_slice() {
        [device] => Ok((*device).clone()),
        [] if devices.is_empty() => Err("No emulators or simulators".to_owned()),
        [] => {
            let known: Vec<String> = devices
                .iter()
                .map(|d| format!("{} ({})", d.name, d.id))
                .collect();
            Err(format!("No device {named}; devices: {}", known.join(", ")))
        }
        several => {
            let ids: Vec<String> = several.iter().map(|d| d.id.to_string()).collect();
            Err(format!(
                "Several devices are called {named}: give one of the ids {}",
                ids.join(", ")
            ))
        }
    }
}

/// A `device/1` stream, and for this machine the task that serves it.
struct Link {
    tx: LinkTx,
    rx: LinkRx,
    _here: Option<AbortOnDropHandle<()>>,
}

// ----- Attached devices -----

/// The devices the agents look at and drive, attached until they idle,
/// the least recently used first.
#[derive(Default)]
pub(crate) struct Screens {
    open: Mutex<Vec<Attached>>,
}

/// An attached device: its machine, the id of the device and the name or id
/// it was called by.
struct Attached {
    machine: EndpointId,
    id: String,
    called: String,
    viewer: Viewer,
}

/// One attached device: where its calls go, whether it takes input, and
/// the task that holds its stream, which ends when it idles or this is
/// dropped.
#[derive(Clone)]
struct Viewer {
    asks: mpsc::Sender<Ask>,
    takes_input: bool,
    _task: std::sync::Arc<AbortOnDropHandle<()>>,
}

enum Ask {
    /// The newest frame, or the first one to come.
    Shot(oneshot::Sender<Result<Frame, String>>),
    /// Input to send, each with the pause after it.
    Input(Vec<(Input, Duration)>, oneshot::Sender<()>),
}

impl Screens {
    /// The attached device `at` names, attached now when it is not, and
    /// the device it is.
    async fn viewer(&self, reach: &Reach<'_>, at: &Target) -> Result<(Viewer, SessionDevice)> {
        let machine = machine(reach.mesh, &at.machine)?;
        let called = at.device.trim();

        if let Some(seen) = self.touch(machine.id, called) {
            return Ok(seen);
        }

        let (machine, device) = reach.find(at).await?;

        // Attached by id earlier and called by name now: reuse that viewer.
        if let Some(seen) = self.touch(machine.id, &device.id.0) {
            return Ok(seen);
        }

        ensure_on(&machine, &device)?;

        let mut link = reach.open(&machine).await?;
        link.tx
            .send(&Request::Attach {
                id: device.id.clone(),
                max_side: SHOT_SIDE,
            })
            .await?;
        let viewer = Viewer::start(link, IDLE, takes_input(&device));
        let used = used(&machine, &device);
        self.attach(Attached {
            machine: machine.id,
            id: device.id.0,
            called: called.to_owned(),
            viewer: viewer.clone(),
        });

        Ok((viewer, used))
    }

    /// The live viewer of the device called `called`, now the most recently
    /// used, and the device it is.
    fn touch(&self, machine: EndpointId, called: &str) -> Option<(Viewer, SessionDevice)> {
        let mut open = crate::lock(&self.open);
        open.retain(|a| !a.viewer.asks.is_closed());
        let at = open
            .iter()
            .position(|a| a.machine == machine && (a.called == called || a.id == called))?;
        let attached = open.remove(at);
        let seen = (
            attached.viewer.clone(),
            SessionDevice {
                machine,
                device: DeviceId(attached.id.clone()),
            },
        );
        open.push(attached);

        Some(seen)
    }

    /// Keeps `attached` as the most recently used, detaching the least
    /// recently used ones beyond `MAX_ATTACHED`.
    fn attach(&self, attached: Attached) {
        let mut open = crate::lock(&self.open);
        open.retain(|a| {
            !a.viewer.asks.is_closed() && (a.machine != attached.machine || a.id != attached.id)
        });
        open.push(attached);

        while open.len() > MAX_ATTACHED {
            open.remove(0);
        }
    }

    /// Detaches from the device with `id` on `machine`, and from any device
    /// called `called`.
    fn close(&self, machine: EndpointId, id: &str, called: &str) {
        let called = called.trim();
        crate::lock(&self.open)
            .retain(|a| a.machine != machine || (a.id != id && a.called != called));
    }
}

impl Viewer {
    /// Follows the attached stream of `link` until no call came for
    /// `idle`, the device's view ends, or every copy of the viewer is gone.
    fn start(link: Link, idle: Duration, takes_input: bool) -> Self {
        let (asks, taken) = mpsc::channel(8);
        let task = tokio::spawn(view(link, taken, idle));

        Self {
            asks,
            takes_input,
            _task: std::sync::Arc::new(AbortOnDropHandle::new(task)),
        }
    }

    async fn shot(&self) -> Result<Frame> {
        let (reply, answer) = oneshot::channel();
        self.asks
            .send(Ask::Shot(reply))
            .await
            .map_err(|_| anyhow::anyhow!(VIEW_STOPPED))?;

        match timeout(SHOT_LIMIT, answer).await {
            Ok(Ok(Ok(frame))) => Ok(frame),
            Ok(Ok(Err(reason))) => bail!(reason),
            Ok(Err(_)) => bail!(VIEW_STOPPED),
            Err(_) => bail!("the device showed no screen in time"),
        }
    }

    async fn input(&self, inputs: Vec<(Input, Duration)>) -> Result<()> {
        if !self.takes_input {
            bail!("Beep cannot tap or type on an iPhone yet: ask the user to.");
        }

        let (sent, answer) = oneshot::channel();
        self.asks
            .send(Ask::Input(inputs, sent))
            .await
            .map_err(|_| anyhow::anyhow!(VIEW_STOPPED))?;

        answer.await.map_err(|_| anyhow::anyhow!(VIEW_STOPPED))
    }
}

/// Keeps the newest frame of an attached stream and answers the asks,
/// until `idle` passes without one, the view ends or the asks close.
async fn view(mut link: Link, mut asks: mpsc::Receiver<Ask>, idle: Duration) {
    let mut newest: Option<Frame> = None;
    let mut waiting: Vec<oneshot::Sender<Result<Frame, String>>> = Vec::new();
    let mut deadline = Instant::now() + idle;

    let ended = loop {
        tokio::select! {
            () = sleep_until(deadline) => break "the device's view idled".to_owned(),
            ask = asks.recv() => {
                deadline = Instant::now() + idle;

                match ask {
                    None => break "the device's view closed".to_owned(),
                    Some(Ask::Shot(reply)) => match &newest {
                        Some(frame) => drop(reply.send(Ok(frame.clone()))),
                        None => waiting.push(reply),
                    },
                    Some(Ask::Input(inputs, sent)) => {
                        for (input, pause) in inputs {
                            if let Err(e) = link.tx.send(&Request::Input { input }).await {
                                break_with(&mut waiting, &format!("{e:#}"));
                                return;
                            }

                            sleep(pause).await;
                        }

                        let _ = sent.send(());
                    }
                }
            }
            reply = link.rx.recv::<Reply>() => match reply {
                Ok(Some(Reply::Frame(frame))) => {
                    for reply in waiting.drain(..) {
                        let _ = reply.send(Ok(frame.clone()));
                    }

                    newest = Some(frame);
                }
                Ok(Some(Reply::Ended { reason } | Reply::Failed { reason })) => break reason,
                Ok(Some(_)) => {}
                Ok(None) => break VIEW_STOPPED.to_owned(),
                Err(e) => break format!("{e:#}"),
            },
        }
    };

    break_with(&mut waiting, &ended);
}

/// Tells the calls waiting for a frame why none comes.
fn break_with(waiting: &mut Vec<oneshot::Sender<Result<Frame, String>>>, reason: &str) {
    for reply in waiting.drain(..) {
        let _ = reply.send(Err(reason.to_owned()));
    }
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_devices.rs"]
mod tests;
