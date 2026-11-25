//! `device/1`: the machine's Android emulators (`android`), Android phones
//! (`phone`), iOS simulators (`ios`) and iPhones (`iphone`, both on macOS), listed, started and stopped, and shown live
//! to a desktop that taps and types on them; their logs, screen contents,
//! installs, launches and resets for agents (`adb` for Android); and which
//! of those platforms a checkout builds for (`platforms`).

mod adb;
mod android;
// The companion client uses Unix sockets, so Linux can also type-check it
// and run its pure protocol and input tests without an Apple runtime.
#[cfg(any(target_os = "macos", all(test, unix)))]
#[cfg_attr(all(test, not(target_os = "macos")), allow(dead_code))]
mod ios;
#[cfg(any(target_os = "macos", all(test, unix)))]
#[cfg_attr(all(test, not(target_os = "macos")), allow(dead_code))]
mod iphone;
mod phone;
mod platforms;
pub(crate) mod video;

use crate::agents::{Agents, reach::Access};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    device::{
        Device, DeviceId, DeviceState, Element, Frame, Input, MAX_LOG_LINES, MAX_SIDE, Platform,
        Reply, Request,
    },
};
use beep_proto::{node::events::Topic, watched};
use futures::{FutureExt, Stream, StreamExt};
use std::{
    future::Future,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use strum::FromRepr;
use tokio::{
    select,
    sync::watch,
    time::{Instant, sleep, sleep_until},
};
use tokio_util::task::AbortOnDropHandle;
use tonic::Status;

pub use beep_proto::node::device::PROTOCOL;
pub use phone::track_phones;

/// The shortest interval between starting frames: about 30 a second.
const FRAME_GAP: Duration = Duration::from_millis(33);
/// Off macOS, where no iOS simulator is listed.
#[cfg(not(target_os = "macos"))]
const NO_SIMULATORS: &str = "iOS simulators only run on a Mac.";
/// The smallest frame a desktop may ask for, so a bad size still shows
/// something.
const MIN_SIDE: u32 = 64;

/// Serves one stream: answers `List`, `Boot`, `Shutdown`, `Logs` and `Platforms`
/// until the desktop closes it, or follows a device once it asks to
/// `Attach`, or the phones once it asks to `FollowPhones`.
pub(crate) async fn serve(mut tx: LinkTx, mut rx: LinkRx, agents: Arc<Agents>) -> Result<()> {
    while let Some(request) = rx.recv::<Request>().await? {
        let reply = match request {
            Request::List {} => Reply::Devices {
                devices: list().await,
            },
            Request::Boot { id } => listed(done(boot(&id).await)),
            Request::Shutdown { id } => listed(done(shutdown(&id).await)),
            Request::Logs { id, lines, filter } => answer(
                logs(&id, lines, &filter)
                    .await
                    .map(|text| Reply::Logs { text }),
            ),
            Request::Ui { id } => answer(ui(&id).await.map(|elements| Reply::Ui { elements })),
            Request::Install { id, path } => done(install(&agents, &id, &path).await),
            Request::Launch { id, target } => done(launch(&id, &target).await),
            Request::Reset { id, app } => done(reset(&id, &app).await),
            Request::Attach { id, max_side } => return attach(tx, rx, &id, max_side).await,
            Request::FollowPhones {} => return phone::follow(tx, rx).await,
            Request::Input { .. } => Reply::Failed {
                reason: "No device is attached.".to_owned(),
            },
            Request::Platforms { root } => answer(
                platforms_in(&agents, &root)
                    .await
                    .map(|platforms| Reply::Platforms { platforms }),
            ),
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

/// The topic that says the device list changed. Released readers name it and
/// cannot decode the list, so it carries no value.
pub(crate) const TOPIC: &str = "devices";
/// The topic that carries the device list. No released reader names it.
pub(crate) const LIST_TOPIC: &str = "device-list";

/// Every emulator, phone and simulator here, Android first.
pub(crate) async fn list() -> Vec<Device> {
    #[cfg(target_os = "macos")]
    let (mut devices, phones, iphones, simulators) =
        tokio::join!(android::list(), phone::list(), iphone::list(), ios::list());
    #[cfg(not(target_os = "macos"))]
    let ((mut devices, phones), iphones, simulators) = (
        tokio::join!(android::list(), phone::list()),
        Vec::new(),
        Vec::new(),
    );

    devices.extend(phones);
    devices.extend(iphones);
    devices.extend(simulators);
    devices
}

/// How often the list is read again while a device boots: the emulator says
/// it is up to no one but its own port.
const BOOTING_EVERY: Duration = Duration::from_secs(2);
/// How often it is read again while an iOS simulator is booted or an iPhone
/// is plugged in, which end outside Beep with no event. A shut-down simulator
/// starts with a boot here, which rings `CHANGED`.
const SIMULATORS_EVERY: Duration = Duration::from_secs(5);

/// Rung when adb, or a boot or shutdown here, may have changed the list.
static CHANGED: LazyLock<watch::Sender<()>> = LazyLock::new(|| watch::Sender::new(()));

/// The list as `Follow` last read it: none until it read, or after the last
/// client stopped following.
static LISTED: LazyLock<watch::Sender<Option<Vec<Device>>>> =
    LazyLock::new(|| watch::Sender::new(None));

/// The clients that follow the list, and the task that reads it for them.
static FOLLOWING: Mutex<Following> = Mutex::new(Following {
    clients: 0,
    task: None,
});

struct Following {
    clients: usize,
    task: Option<AbortOnDropHandle<()>>,
}

/// Held while a client follows the device topics: one task reads the list
/// when adb or a boot says it may have changed, and on a timer only while a
/// simulator is listed or a device boots, which nothing announces. It stores
/// the list before it pings `devices`, so a reader gets the value that was
/// read. Stops with the last holder.
pub(crate) struct Follow;

impl Follow {
    pub(crate) fn new() -> Self {
        let mut following = crate::lock(&FOLLOWING);
        following.clients += 1;

        if following.task.is_none() {
            let following_task = following_list(
                list,
                &LISTED,
                CHANGED.subscribe(),
                || crate::events::system().changed(Topic::new(TOPIC)),
            );
            following.task = Some(AbortOnDropHandle::new(tokio::spawn(following_task)));
        }

        Self
    }
}

impl Drop for Follow {
    fn drop(&mut self) {
        let mut following = crate::lock(&FOLLOWING);
        following.clients -= 1;

        if following.clients == 0 {
            following.task = None;
            LISTED.send_replace(None);
        }
    }
}

/// The list as `Follow` stores it: none until it read, or after the last
/// client stopped following. Readers follow it and never ask the module.
pub(crate) fn stored() -> watch::Receiver<Option<Vec<Device>>> {
    LISTED.subscribe()
}

/// Reads the list at the start and after each change of `changed`, stores it
/// in `listed` and calls `ping` when it differs from the one before.
async fn following_list<Read, Reading>(
    read: Read,
    listed: &'static watch::Sender<Option<Vec<Device>>>,
    mut changed: watch::Receiver<()>,
    ping: impl Fn() + Send + 'static,
) where
    Read: Fn() -> Reading + Send + 'static,
    Reading: Future<Output = Vec<Device>> + Send,
{
    loop {
        // Seen before the read, so a change during it reads again.
        changed.mark_unchanged();
        let now = read().await;
        let every = [
            (now.iter().any(|device| device.state == DeviceState::Booting), BOOTING_EVERY),
            (
                now.iter().any(|device| {
                    device.platform == Platform::Ios && (device.phone || device.state == DeviceState::On)
                }),
                SIMULATORS_EVERY,
            ),
        ]
        .into_iter()
        .find_map(|(needed, every)| needed.then_some(every));
        let mut moved = false;
        listed.send_if_modified(|last| {
            moved = last.as_ref().is_some_and(|last| *last != now);
            let changed = last.as_ref() != Some(&now);
            *last = Some(now);
            changed
        });

        if moved {
            ping();
        }

        select! {
            result = changed.changed() => {
                if result.is_err() {
                    return;
                }
            }
            () = sleep(every.unwrap_or_default()), if every.is_some() => {}
        }
    }
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

/// Device `id`'s newest `lines` log lines containing `filter`: logcat for
/// Android, the unified log of the last minutes for an iOS simulator.
async fn logs(id: &DeviceId, lines: u32, filter: &str) -> Result<String> {
    let text = match reach(id).await? {
        Reached::Adb(serial) => adb::logcat(&serial).await?,
        #[cfg(target_os = "macos")]
        Reached::Simulator(udid) => ios::logs(&udid).await?,
        #[cfg(target_os = "macos")]
        Reached::Iphone(_) => anyhow::bail!("Beep cannot read an iPhone's log yet."),
    };

    Ok(newest_lines(&text, filter, lines.min(MAX_LOG_LINES)))
}

/// What device `id`'s screen shows.
async fn ui(id: &DeviceId) -> Result<Vec<Element>> {
    match reach(id).await? {
        Reached::Adb(serial) => adb::ui(&serial).await,
        #[cfg(target_os = "macos")]
        Reached::Simulator(udid) => ios::ui(&udid).await,
        #[cfg(target_os = "macos")]
        Reached::Iphone(_) => {
            anyhow::bail!("Beep cannot read an iPhone's screen contents: take a screenshot.")
        }
    }
}

/// Installs the app at `path`, a file the caller may read, on device `id`.
async fn install(agents: &Agents, id: &DeviceId, path: &str) -> Result<()> {
    let real = agents.reachable(path, Access::Read).await?;

    match reach(id).await? {
        Reached::Adb(serial) => adb::install(&serial, &real).await,
        #[cfg(target_os = "macos")]
        Reached::Simulator(udid) => ios::install(&udid, &real).await,
        #[cfg(target_os = "macos")]
        Reached::Iphone(udid) => iphone::install(&udid, &real).await,
    }
}

async fn launch(id: &DeviceId, target: &str) -> Result<()> {
    match reach(id).await? {
        Reached::Adb(serial) => adb::launch(&serial, target).await,
        #[cfg(target_os = "macos")]
        Reached::Simulator(udid) => ios::launch(&udid, target).await,
        #[cfg(target_os = "macos")]
        Reached::Iphone(udid) => iphone::launch(&udid, target).await,
    }
}

async fn reset(id: &DeviceId, app: &str) -> Result<()> {
    match reach(id).await? {
        Reached::Adb(serial) => adb::reset(&serial, app).await,
        #[cfg(target_os = "macos")]
        Reached::Simulator(_) | Reached::Iphone(_) => {
            anyhow::bail!("iOS cannot clear an app: install it again instead.")
        }
    }
}

/// How a device that is on takes commands.
enum Reached {
    /// By this adb serial, a phone's own or an emulator's.
    Adb(DeviceId),
    /// A simulator, by its UDID.
    #[cfg(target_os = "macos")]
    Simulator(DeviceId),
    /// An iPhone or iPad, by its UDID.
    #[cfg(target_os = "macos")]
    Iphone(DeviceId),
}

/// How device `id` takes commands, once it is on.
async fn reach(id: &DeviceId) -> Result<Reached> {
    let device = find(id).await?;

    if device.state != DeviceState::On {
        anyhow::bail!("{} is not on.", device.name);
    }

    match device.platform {
        Platform::Android if device.phone => Ok(Reached::Adb(device.id)),
        Platform::Android => Ok(Reached::Adb(android::serial(id).await?)),
        #[cfg(target_os = "macos")]
        Platform::Ios if device.phone => Ok(Reached::Iphone(device.id)),
        #[cfg(target_os = "macos")]
        Platform::Ios => Ok(Reached::Simulator(device.id)),
        #[cfg(not(target_os = "macos"))]
        Platform::Ios => anyhow::bail!(NO_SIMULATORS),
    }
}

/// The last `lines` lines of `text` that contain `filter`, in any case.
fn newest_lines(text: &str, filter: &str, lines: u32) -> String {
    let filter = filter.to_lowercase();
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| filter.is_empty() || line.to_lowercase().contains(&filter))
        .collect();
    let lines = usize::try_from(lines).unwrap_or(usize::MAX);

    kept[kept.len().saturating_sub(lines)..].join("\n")
}

/// `device`, unless it is a phone, which only its owner starts and stops.
fn on_its_own(device: Device) -> Result<Device> {
    if device.phone {
        anyhow::bail!(
            "{} is a phone: only you can turn it on or off.",
            device.name
        );
    }

    Ok(device)
}

/// `reply`, or why there is none.
fn answer(reply: Result<Reply>) -> Reply {
    reply.unwrap_or_else(|e| Reply::Failed {
        reason: format!("{e:#}"),
    })
}

fn done(result: Result<()>) -> Reply {
    answer(result.map(|()| Reply::Done {}))
}

/// Says the device list changed when `reply` says a boot or shutdown went through.
fn listed(reply: Reply) -> Reply {
    if matches!(reply, Reply::Done {}) {
        CHANGED.send_replace(());
    }

    reply
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
    #[cfg(any(target_os = "macos", all(test, unix)))]
    Ios(Box<ios::Controls>),
    /// An iPhone, which takes no input.
    #[cfg(any(target_os = "macos", all(test, unix)))]
    Iphone,
}

impl Controls {
    async fn send(&mut self, input: Input) -> Result<()> {
        match self {
            Self::Android(controls) => controls.send(input).await,
            Self::Phone(controls) => controls.send(input).await,
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Self::Ios(controls) => controls.send(input).await,
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Self::Iphone => anyhow::bail!(iphone::NO_INPUT),
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
        screen,
        mut controls,
        _work,
    } = live;

    let inputs = async {
        while let Some(request) = rx.recv::<Request>().await? {
            match request {
                Request::Input { input } => {
                    if let Err(e) = controls.send(input).await {
                        log::warn!("device {id}: input not taken: {e:#}");
                    }
                }
                other => log::debug!("device {id}: ignoring {other:?} while attached"),
            }
        }

        Ok(())
    };

    stream(tx, screen, inputs).await
}

/// Each direction progresses independently. Ending either loop drops the other,
/// including any write or input operation still in progress.
async fn stream(
    mut tx: LinkTx,
    mut screen: watch::Receiver<Screen>,
    inputs: impl Future<Output = Result<()>>,
) -> Result<()> {
    let frames = async {
        loop {
            if screen.changed().await.is_err() {
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
    };

    tokio::select! {
        result = frames => result,
        result = inputs => result,
    }
}

/// Starts following device `id`, which must be on or booting.
async fn open(id: &DeviceId, max_side: u32) -> Result<Live> {
    let device = find(id).await?;

    if device.state == DeviceState::Off {
        anyhow::bail!("{} is off.", device.name);
    }

    match device.platform {
        Platform::Android if device.phone => phone::attach(id, max_side).await,
        Platform::Android => android::attach(id, max_side).await,
        #[cfg(target_os = "macos")]
        Platform::Ios if device.phone => iphone::attach(id, max_side).await,
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
    mut stream: impl Stream<Item = Result<T, Status>> + Unpin,
    screen: watch::Sender<Screen>,
    mut show: impl AsyncFnMut(T) -> Result<Option<Frame>>,
    ended: &str,
) {
    let mut next_frame = Instant::now();
    let mut next_shown = next_frame;
    let reason = loop {
        sleep_until(next_frame).await;
        let message = match newest(&mut stream).await {
            Ok(Some(message)) => message,
            Ok(None) => break ended.to_owned(),
            Err(status) => break format!("{ended} ({})", status.message()),
        };

        next_frame = Instant::now() + FRAME_GAP;

        match show(message).await {
            Ok(Some(frame)) => {
                // A fast frame after a slow conversion must not cause a burst.
                sleep_until(next_shown).await;
                screen.send_replace(Screen::Frame(frame));
                next_shown = Instant::now() + FRAME_GAP;
            }
            Ok(None) => {}
            Err(e) => log::warn!("a device frame was not shown: {e:#}"),
        }
    };
    screen.send_replace(Screen::Ended(reason));
}

/// The next message of `stream`, skipping to the newest of those already
/// waiting; `None` once it ended.
async fn newest<T>(
    stream: &mut (impl Stream<Item = Result<T, Status>> + Unpin),
) -> Result<Option<T>, Status> {
    let Some(mut message) = stream.next().await.transpose()? else {
        return Ok(None);
    };

    while let Some(waiting) = stream.next().now_or_never() {
        match waiting.transpose()? {
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
