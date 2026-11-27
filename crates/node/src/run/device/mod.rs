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
mod screen;
pub(crate) mod video;

use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    device::{
        Device, DeviceId, DeviceState, Element, Input, MAX_LOG_LINES, MAX_SIDE, Platform, Reply,
        Request,
    },
};
use beep_proto::{node::events::Topic, watched};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{select, sync::watch, time::sleep};
use tokio_util::task::AbortOnDropHandle;

pub use beep_proto::node::device::PROTOCOL;
pub use phone::track_phones;

use screen::{CHANGED, Live, Screen};

#[cfg(not(target_os = "macos"))]
const NO_SIMULATORS: &str = "iOS simulators only run on a Mac.";
/// The smallest frame a desktop may ask for, so a bad size still shows
/// something.
const MIN_SIDE: u32 = 64;

/// One stream, answered request by request. A request that names a folder
/// or file (`named`) is answered with the path its caller checked.
pub(crate) struct Served {
    tx: LinkTx,
    rx: LinkRx,
}

/// The folder or file `request` names, for the caller to check and resolve
/// for reading before it answers.
pub(crate) fn named(request: &Request) -> Option<&str> {
    match request {
        Request::Install { path, .. } => Some(path),
        Request::Platforms { root } => Some(root),
        _ => None,
    }
}

impl Served {
    pub(crate) fn new(tx: LinkTx, rx: LinkRx) -> Self {
        Self { tx, rx }
    }

    /// The next request, or none once the desktop closes the stream.
    pub(crate) async fn next(&mut self) -> Result<Option<Request>> {
        self.rx.recv::<Request>().await
    }

    /// Answers `request`: `List`, `Boot`, `Shutdown`, `Logs` and `Platforms`
    /// until the desktop closes it, or follows a device once it asks to
    /// `Attach`, or the phones once it asks to `FollowPhones`. `path` is
    /// what `named(&request)` names, checked and resolved. The stream goes on
    /// unless the request handed it to a follower.
    pub(crate) async fn answer(
        mut self,
        request: Request,
        path: Option<Result<PathBuf>>,
    ) -> Result<Option<Self>> {
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
            Request::Install { id, .. } => done(match checked(path) {
                Ok(real) => install(&id, &real).await,
                Err(e) => Err(e),
            }),
            Request::Launch { id, target } => done(launch(&id, &target).await),
            Request::Reset { id, app } => done(reset(&id, &app).await),
            Request::Attach { id, max_side } => {
                attach(self.tx, self.rx, &id, max_side).await?;
                return Ok(None);
            }
            Request::FollowPhones {} => {
                phone::follow(self.tx, self.rx).await?;
                return Ok(None);
            }
            Request::Input { .. } => Reply::Failed {
                reason: "No device is attached.".to_owned(),
            },
            Request::Platforms { .. } => answer(
                platforms_in(checked(path))
                    .await
                    .map(|platforms| Reply::Platforms { platforms }),
            ),
            other => Reply::Failed {
                reason: format!("Unknown request {other:?}."),
            },
        };
        self.tx.send(&reply).await?;

        Ok(Some(self))
    }
}

/// The checked path of a request that names one.
fn checked(path: Option<Result<PathBuf>>) -> Result<PathBuf> {
    path.context("the request's path was not checked")?
}

/// The platforms the app in the checked folder `root` builds for.
async fn platforms_in(root: Result<PathBuf>) -> Result<Vec<Platform>> {
    let real = root?;

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

/// The list as `Follow` last read it: none until it read, or after the last
/// client stopped following.
static LISTED: LazyLock<watch::Sender<Option<Vec<Device>>>> =
    LazyLock::new(|| watch::Sender::new(None));

/// The follower task that may store the list, or 0 when none may. Set before
/// `LISTED` is cleared and checked inside the store, so a task that was mid-read
/// when its last client left cannot write an old list back.
static ACTIVE: AtomicU64 = AtomicU64::new(0);
static TASKS: AtomicU64 = AtomicU64::new(0);

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
            let mine = TASKS.fetch_add(1, Ordering::Relaxed) + 1;
            ACTIVE.store(mine, Ordering::SeqCst);
            let following_task = following_list(
                list,
                &LISTED,
                move || ACTIVE.load(Ordering::SeqCst) == mine,
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
            ACTIVE.store(0, Ordering::SeqCst);
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
    current: impl Fn() -> bool + Send + 'static,
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
            (
                now.iter()
                    .any(|device| device.state == DeviceState::Booting),
                BOOTING_EVERY,
            ),
            (
                now.iter().any(|device| {
                    device.platform == Platform::Ios
                        && (device.phone || device.state == DeviceState::On)
                }),
                SIMULATORS_EVERY,
            ),
        ]
        .into_iter()
        .find_map(|(needed, every)| needed.then_some(every));
        let mut moved = false;
        listed.send_if_modified(|last| {
            // Checked under the sender's lock, which clearing the list takes too.
            if !current() {
                return false;
            }

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

/// Device `id` as the list `Follow` keeps showed it, when that had it on, so a
/// command for a device the caller just listed does not read every platform
/// again: a Mac's `simctl` and `devicectl` take up to 20 s. Reads the list
/// when the device was not listed on, which also catches one that came up since.
async fn find_on<Read, Reading>(
    id: &DeviceId,
    stored: &watch::Receiver<Option<Vec<Device>>>,
    read: Read,
) -> Result<Device>
where
    Read: FnOnce() -> Reading,
    Reading: Future<Output = Vec<Device>>,
{
    let known = watched::read(stored, |devices| {
        devices.as_ref().and_then(|devices| {
            devices
                .iter()
                .find(|device| device.id == *id && device.state == DeviceState::On)
                .cloned()
        })
    });

    if let Some(device) = known {
        return Ok(device);
    }

    read()
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

/// Installs the app at `real`, a file the caller may read, on device `id`.
async fn install(id: &DeviceId, real: &Path) -> Result<()> {
    match reach(id).await? {
        Reached::Adb(serial) => adb::install(&serial, real).await,
        #[cfg(target_os = "macos")]
        Reached::Simulator(udid) => ios::install(&udid, real).await,
        #[cfg(target_os = "macos")]
        Reached::Iphone(udid) => iphone::install(&udid, real).await,
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
    reach_with(id, &stored(), list).await
}

async fn reach_with<Read, Reading>(
    id: &DeviceId,
    stored: &watch::Receiver<Option<Vec<Device>>>,
    read: Read,
) -> Result<Reached>
where
    Read: FnOnce() -> Reading,
    Reading: Future<Output = Vec<Device>>,
{
    let device = find_on(id, stored, read).await?;

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

enum Controls {
    Android(Box<android::Controls>),
    Phone(Box<phone::Controls>),
    #[cfg(any(target_os = "macos", all(test, unix)))]
    #[cfg_attr(all(test, not(target_os = "macos")), allow(dead_code))]
    Ios(Box<ios::Controls>),
    /// An iPhone, which takes no input.
    #[cfg(any(target_os = "macos", all(test, unix)))]
    #[cfg_attr(all(test, not(target_os = "macos")), allow(dead_code))]
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
async fn open(id: &DeviceId, max_side: u32) -> Result<Live<Controls>> {
    let device = find(id).await?;

    if device.state == DeviceState::Off {
        anyhow::bail!("{} is off.", device.name);
    }

    match device.platform {
        Platform::Android if device.phone => phone::attach(id, max_side)
            .await
            .map(|live| live.map(|controls| Controls::Phone(Box::new(controls)))),
        Platform::Android => android::attach(id, max_side)
            .await
            .map(|live| live.map(|controls| Controls::Android(Box::new(controls)))),
        #[cfg(target_os = "macos")]
        Platform::Ios if device.phone => iphone::attach(id, max_side)
            .await
            .map(|live| live.map(|()| Controls::Iphone)),
        #[cfg(target_os = "macos")]
        Platform::Ios => ios::attach(id, max_side)
            .await
            .map(|live| live.map(|controls| Controls::Ios(Box::new(controls)))),
        #[cfg(not(target_os = "macos"))]
        Platform::Ios => anyhow::bail!(NO_SIMULATORS),
    }
}

#[cfg(test)]
#[path = "../../tests/run_device.rs"]
mod tests;
