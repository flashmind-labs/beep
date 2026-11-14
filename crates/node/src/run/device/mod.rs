//! `device/1`: the machine's Android emulators (`android`), phones (`phone`) and iOS
//! simulators (`ios`, on macOS), listed, started and stopped, and shown live
//! to a desktop that taps and types on them; and which of those platforms a
//! checkout builds for (`platforms`).

mod android;
#[cfg(target_os = "macos")]
mod ios;
mod phone;
mod platforms;

use crate::agents::{Agents, reach::Access};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    device::{Device, DeviceId, DeviceState, Frame, Input, MAX_SIDE, Platform, Reply, Request},
};
use beep_proto::watched;
use futures::FutureExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use strum::FromRepr;
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;
use tonic::{Status, Streaming};

pub use beep_proto::node::device::PROTOCOL;
pub(crate) use phone::track;

/// The shortest wait between two frames: about 30 a second.
const FRAME_GAP: Duration = Duration::from_millis(33);
/// Off macOS, where no iOS simulator is listed.
#[cfg(not(target_os = "macos"))]
const NO_SIMULATORS: &str = "iOS simulators only run on a Mac.";
/// The smallest frame a desktop may ask for, so a bad size still shows
/// something.
const MIN_SIDE: u32 = 64;

/// Serves one stream: answers `List`, `Boot`, `Shutdown` and `Platforms`
/// until the desktop closes it, or follows a device once it asks to
/// `Attach`, or the phones once it asks to `FollowPhones`.
pub(crate) async fn serve(mut tx: LinkTx, mut rx: LinkRx, agents: Arc<Agents>) -> Result<()> {
    while let Some(request) = rx.recv::<Request>().await? {
        let reply = match request {
            Request::List {} => Reply::Devices {
                devices: list().await,
            },
            Request::Boot { id } => done(boot(&id).await),
            Request::Shutdown { id } => done(shutdown(&id).await),
            Request::Attach { id, max_side } => return attach(tx, rx, &id, max_side).await,
            Request::FollowPhones {} => return phone::follow(tx, rx).await,
            Request::Input { .. } => Reply::Failed {
                reason: "No device is attached.".to_owned(),
            },
            Request::Platforms { root } => match platforms_in(&agents, &root).await {
                Ok(platforms) => Reply::Platforms { platforms },
                Err(e) => Reply::Failed {
                    reason: format!("{e:#}"),
                },
            },
            other => Reply::Failed {
                reason: format!("Unknown request {other:?}."),
            },
        };
        tx.send(&reply).await?;
    }

    Ok(())
}

/// The platforms the app in folder `root` builds for, once `root` is a
/// folder the desktop may read.
async fn platforms_in(agents: &Agents, root: &str) -> Result<Vec<Platform>> {
    let real = agents.reachable(root, Access::Read).await?;

    tokio::task::spawn_blocking(move || platforms::platforms(&real))
        .await
        .context("the platform check stopped")
}

/// Every emulator, phone and simulator here, Android first.
async fn list() -> Vec<Device> {
    #[cfg(target_os = "macos")]
    let (mut devices, phones, simulators) =
        tokio::join!(android::list(), phone::list(), ios::list());
    #[cfg(not(target_os = "macos"))]
    let ((mut devices, phones), simulators) =
        (tokio::join!(android::list(), phone::list()), Vec::new());

    devices.extend(phones);
    devices.extend(simulators);
    devices
}

/// Device `id`, from the list.
async fn find(id: &DeviceId) -> Result<Device> {
    list()
        .await
        .into_iter()
        .find(|device| device.id == *id)
        .with_context(|| format!("No device {id} on this machine."))
}

async fn boot(id: &DeviceId) -> Result<()> {
    match on_its_own(find(id).await?)?.platform {
        Platform::Android => android::boot(id).await,
        #[cfg(target_os = "macos")]
        Platform::Ios => ios::boot(id).await,
        #[cfg(not(target_os = "macos"))]
        Platform::Ios => anyhow::bail!(NO_SIMULATORS),
    }
}

async fn shutdown(id: &DeviceId) -> Result<()> {
    match on_its_own(find(id).await?)?.platform {
        Platform::Android => android::shutdown(id).await,
        #[cfg(target_os = "macos")]
        Platform::Ios => ios::shutdown(id).await,
        #[cfg(not(target_os = "macos"))]
        Platform::Ios => anyhow::bail!(NO_SIMULATORS),
    }
}

/// `device`, unless it is a phone, which only its owner starts and stops.
fn on_its_own(device: Device) -> Result<Device> {
    if device.phone {
        anyhow::bail!("{} is a phone: only you can turn it on or off.", device.name);
    }

    Ok(device)
}

fn done(result: Result<()>) -> Reply {
    match result {
        Ok(()) => Reply::Done {},
        Err(e) => Reply::Failed {
            reason: format!("{e:#}"),
        },
    }
}

/// What a device shows, newest only.
#[derive(Debug, Clone)]
enum Screen {
    /// No frame yet.
    Starting,
    Frame(Frame),
    /// The view cannot go on, and why.
    Ended(String),
}

/// A device being shown: its screen, kept current by `_work`, and where
/// input goes.
struct Live {
    screen: watch::Receiver<Screen>,
    controls: Controls,
    _work: AbortOnDropHandle<()>,
}

enum Controls {
    Android(Box<android::Controls>),
    Phone(Box<phone::Controls>),
    #[cfg(target_os = "macos")]
    Ios(Box<ios::Controls>),
}

impl Controls {
    async fn send(&mut self, input: Input) -> Result<()> {
        match self {
            Self::Android(controls) => controls.send(input).await,
            Self::Phone(controls) => controls.send(input).await,
            #[cfg(target_os = "macos")]
            Self::Ios(controls) => controls.send(input).await,
        }
    }
}

/// Sends device `id`'s frames as they change and passes the desktop's input
/// on, until the device stops or the desktop closes the stream.
async fn attach(mut tx: LinkTx, mut rx: LinkRx, id: &DeviceId, max_side: u32) -> Result<()> {
    let max_side = max_side.clamp(MIN_SIDE, MAX_SIDE);
    let live = match open(id, max_side).await {
        Ok(live) => live,
        Err(e) => {
            let reason = format!("{e:#}");
            return tx.send(&Reply::Failed { reason }).await;
        }
    };
    let Live {
        mut screen,
        mut controls,
        _work,
    } = live;

    loop {
        tokio::select! {
            changed = screen.changed() => {
                if changed.is_err() {
                    let reason = "The view stopped.".to_owned();
                    return tx.send(&Reply::Ended { reason }).await;
                }

                let shown = watched::read_and_update(&mut screen, Clone::clone);

                match shown {
                    Screen::Starting => {}
                    Screen::Frame(frame) => tx.send(&Reply::Frame(frame)).await?,
                    Screen::Ended(reason) => return tx.send(&Reply::Ended { reason }).await,
                }
            }
            request = rx.recv::<Request>() => match request? {
                Some(Request::Input { input }) => {
                    if let Err(e) = controls.send(input).await {
                        log::warn!("device {id}: input not taken: {e:#}");
                    }
                }
                Some(other) => log::debug!("device {id}: ignoring {other:?} while attached"),
                None => return Ok(()),
            },
        }
    }
}

/// Starts following device `id`, which must be on or booting.
async fn open(id: &DeviceId, max_side: u32) -> Result<Live> {
    let device = find(id).await?;

    if device.state == DeviceState::Off {
        anyhow::bail!("{} is off.", device.name);
    }

    if device.phone {
        return phone::attach(id, max_side).await;
    }

    match device.platform {
        Platform::Android => android::attach(id, max_side).await,
        #[cfg(target_os = "macos")]
        Platform::Ios => ios::attach(id, max_side).await,
        #[cfg(not(target_os = "macos"))]
        Platform::Ios => anyhow::bail!(NO_SIMULATORS),
    }
}

/// Shows what `stream` sends, newest first and at most one frame per
/// `FRAME_GAP`, until it ends, then ends `screen` with `ended`. `show` makes
/// a frame of a message, or `None` when the screen did not change.
async fn follow<T>(
    mut stream: Streaming<T>,
    screen: watch::Sender<Screen>,
    mut show: impl AsyncFnMut(T) -> Result<Option<Frame>>,
    ended: &str,
) {
    let reason = loop {
        let message = match newest(&mut stream).await {
            Ok(Some(message)) => message,
            Ok(None) => break ended.to_owned(),
            Err(status) => break format!("{ended} ({})", status.message()),
        };

        match show(message).await {
            Ok(Some(frame)) => {
                screen.send_replace(Screen::Frame(frame));
                tokio::time::sleep(FRAME_GAP).await;
            }
            Ok(None) => {}
            Err(e) => log::warn!("a device frame was not shown: {e:#}"),
        }
    };
    screen.send_replace(Screen::Ended(reason));
}

/// The next message of `stream`, skipping to the newest of those already
/// waiting; `None` once it ended.
async fn newest<T>(stream: &mut Streaming<T>) -> Result<Option<T>, Status> {
    let Some(mut message) = stream.message().await? else {
        return Ok(None);
    };

    while let Some(waiting) = stream.message().now_or_never() {
        match waiting? {
            Some(newer) => message = newer,
            None => break,
        }
    }

    Ok(Some(message))
}

/// How a device's screen is turned from upright, as its frames show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, FromRepr)]
#[repr(u8)]
enum Turn {
    #[default]
    Upright,
    /// The upright screen turned a quarter turn counterclockwise: its top
    /// is at the frame's left.
    Left,
    UpsideDown,
    /// The upright screen turned a quarter turn clockwise: its top is at
    /// the frame's right.
    Right,
}

impl Turn {
    /// Where fractions `x`, `y` of a frame turned this way are on the
    /// upright screen, which devices take touches on.
    fn upright(self, x: f32, y: f32) -> (f32, f32) {
        match self {
            Self::Upright => (x, y),
            Self::Left => (1.0 - y, x),
            Self::UpsideDown => (1.0 - x, 1.0 - y),
            Self::Right => (y, 1.0 - x),
        }
    }
}

/// The turn of the newest frame, set as frames come and read by the
/// controls.
#[derive(Debug, Clone, Default)]
struct Turned(Arc<AtomicU8>);

impl Turned {
    fn get(&self) -> Turn {
        Turn::from_repr(self.0.load(Ordering::Relaxed)).unwrap_or_default()
    }

    fn set(&self, turn: Turn) {
        self.0.store(turn as u8, Ordering::Relaxed);
    }
}

/// The point at fractions `x`, `y` of a `width` by `height` screen, each
/// fraction kept within the screen.
fn point(x: f32, y: f32, width: f64, height: f64) -> (f64, f64) {
    let fraction = |f: f32| f64::from(if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) });
    (fraction(x) * width, fraction(y) * height)
}

#[cfg(test)]
#[path = "../../tests/run_device.rs"]
mod tests;
