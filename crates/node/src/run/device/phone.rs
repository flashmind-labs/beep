//! Android phones over adb, by USB or Wi-Fi: listed from `adb devices`,
//! shown from `screencap` and driven with `input`, with `track_phones` saying when
//! one comes or goes. Emulators, which adb lists too, are left to
//! `android`, which reaches them faster.

use super::{
    adb::{CALL_TIMEOUT, NO_ADB, call},
    screen::{CHANGED, FRAME_GAP, Live, Screen, point},
};
use crate::{lock, machine::env, process};
use anyhow::{Context, Result, bail};
use beep_proto::node::device::{
    Button, Device, DeviceId, DeviceState, Frame, Input, Key, MAX_TEXT, Platform, Reply, Request,
    TouchPhase,
};
use beep_proto::node::{LinkRx, LinkTx};
use beep_proto::watched;
use futures::future::join_all;
use image::{ImageFormat, codecs::jpeg::JpegEncoder, imageops::FilterType};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, sync::watch};
use tokio_util::task::AbortOnDropHandle;

/// A finger held this long without moving presses long instead of tapping.
const LONG_PRESS: Duration = Duration::from_millis(500);
/// How far a finger may move, in screen pixels, and still tap.
const TAP_SLOP: f64 = 16.0;
const JPEG_QUALITY: u8 = 80;
const PNG_SIGNATURE: &[u8] = b"\x89PNG";
/// The first wait before following adb again after it stopped, doubled
/// each time up to `TRACK_RETRY_MAX`.
const TRACK_RETRY: Duration = Duration::from_secs(5);
const TRACK_RETRY_MAX: Duration = Duration::from_secs(300);
/// Turns the screen a quarter turn and holds it there, as auto-rotate would.
const ROTATE: &str = "settings put system accelerometer_rotation 0; \
    settings put system user_rotation $(( ($(settings get system user_rotation) + 1) % 4 ))";

/// The phones here as adb last listed them, kept by `track_phones`.
static PHONES: LazyLock<watch::Sender<Vec<Device>>> =
    LazyLock::new(|| watch::Sender::new(Vec::new()));

/// Each phone's Android release, by serial, asked once.
static RELEASES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

/// Whether `follow_adb` keeps `PHONES` current, so a list need not ask adb.
static TRACKING: AtomicBool = AtomicBool::new(false);

/// The phones adb reaches and the user allowed: as `track_phones` last saw
/// them while it follows adb, else as adb says now.
pub(super) async fn list() -> Vec<Device> {
    if TRACKING.load(Ordering::Acquire) {
        return watched::read_sent(&PHONES, Clone::clone);
    }

    read().await
}

/// The phones `adb devices -l` lists.
async fn read() -> Vec<Device> {
    let Some(adb) = env::adb() else {
        return Vec::new();
    };
    let Some(text) = env::output(&adb, &["devices", "-l"]).await else {
        return Vec::new();
    };

    join_all(attached(&text).into_iter().map(|phone| {
        let adb = &adb;
        async move {
            let os = release(adb, &phone.id).await;
            Device {
                id: phone.id,
                name: phone.name,
                platform: Platform::Android,
                state: DeviceState::On,
                os,
                phone: true,
            }
        }
    }))
    .await
}

/// Rings `CHANGED` each time adb sees a phone or emulator come, go or change
/// state, for as long as the node runs, so the list is read then and not on
/// a timer. Without adb it looks for it every `TRACK_RETRY`: adb is a tool
/// outside the node, which does not say when it is installed.
pub async fn track_phones() {
    let adb = found(env::adb).await;
    let mut wait = TRACK_RETRY;

    loop {
        let started = Instant::now();

        if let Err(e) = follow_adb(&adb).await {
            log::debug!("not following adb devices: {e:#}");
        }

        if started.elapsed() > TRACK_RETRY_MAX {
            wait = TRACK_RETRY;
        }

        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(TRACK_RETRY_MAX);
    }
}

/// The path `find` gives once it gives one, asked again every `TRACK_RETRY` until then.
async fn found(find: impl Fn() -> Option<PathBuf>) -> PathBuf {
    loop {
        if let Some(path) = find() {
            return path;
        }

        tokio::time::sleep(TRACK_RETRY).await;
    }
}

/// Runs `adb track-devices`, which prints the device list at once and again
/// on every change, until it stops.
async fn follow_adb(adb: &Path) -> Result<()> {
    let mut command = process::command(adb);
    command
        .arg("track-devices")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let started = command.spawn();
    process::launched(adb, &started);
    let mut child = started.context("could not run adb")?;
    let mut out = child.stdout.take().context("adb gave no output")?;
    let mut read = [0; 4096];

    let result = async {
        while out.read(&mut read).await? > 0 {
            let phones = self::read().await;
            PHONES.send_if_modified(|known| {
                let changed = *known != phones;
                *known = phones;
                changed
            });
            // The list reads `PHONES` from here on, which is current now.
            TRACKING.store(true, Ordering::Release);
            CHANGED.send_replace(());
        }

        anyhow::Ok(())
    }
    .await;
    TRACKING.store(false, Ordering::Release);

    result
}

/// Sends the phones here now and again on every change, until the caller
/// closes the stream.
pub(super) async fn follow(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let mut phones = PHONES.subscribe();

    loop {
        let devices = watched::read_and_update(&mut phones, Clone::clone);
        tx.send(&Reply::Devices { devices }).await?;

        tokio::select! {
            changed = phones.changed() => changed?,
            closed = rx.recv::<Request>() => match closed? {
                Some(other) => log::debug!("phones: ignoring {other:?} while following"),
                None => return Ok(()),
            },
        }
    }
}

/// A phone in `adb devices -l`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Attached {
    id: DeviceId,
    /// Its model, such as "Pixel 9", else its serial.
    name: String,
}

/// The phones `adb devices -l` lists ready, leaving out emulators and
/// phones waiting for the user to allow debugging.
pub(super) fn attached(text: &str) -> Vec<Attached> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let serial = words.next()?;
            let ready = words.next()? == "device";

            if !ready || serial.starts_with("emulator-") {
                return None;
            }

            let model = words.find_map(|word| word.strip_prefix("model:"));
            Some(Attached {
                id: DeviceId(serial.to_owned()),
                name: model.map_or_else(|| serial.to_owned(), |model| model.replace('_', " ")),
            })
        })
        .collect()
}

/// The Android release phone `id` runs, such as "Android 16".
async fn release(adb: &Path, id: &DeviceId) -> Option<String> {
    if let Some(known) = lock(&RELEASES).get(&id.0) {
        return Some(known.clone());
    }

    let out = call(
        adb,
        id,
        &["shell", "getprop", "ro.build.version.release"],
        CALL_TIMEOUT,
    )
    .await;
    let number = String::from_utf8(out.ok()?).ok()?;
    let release = format!("Android {}", number.trim());
    lock(&RELEASES).insert(id.0.clone(), release.clone());
    Some(release)
}

/// Follows phone `id`'s screen, one screenshot after another.
pub(super) async fn attach(id: &DeviceId, max_side: u32) -> Result<Live<Controls>> {
    let adb = env::adb().context(NO_ADB)?;
    let (screen, shown) = watch::channel(Screen::Starting);
    let (size, sized) = watch::channel(None);
    let work = tokio::spawn(capture(adb.clone(), id.clone(), max_side, screen, size));
    let controls = Controls {
        adb,
        id: id.clone(),
        size: sized,
        down: None,
    };

    Ok(Live {
        screen: shown,
        controls,
        _work: AbortOnDropHandle::new(work),
    })
}

/// Controls for native video; the recorder publishes its logical screen size.
pub(super) struct VideoControls {
    pub controls: Controls,
    pub size: watch::Sender<Option<(u32, u32)>>,
}

pub(super) fn video_controls(id: &DeviceId) -> Result<VideoControls> {
    let adb = env::adb().context(NO_ADB)?;
    let (size, sized) = watch::channel(None);
    Ok(VideoControls {
        controls: Controls {
            adb,
            id: id.clone(),
            size: sized,
            down: None,
        },
        size,
    })
}

/// Shows phone `id`'s screen as fast as it gives screenshots, and its size
/// in `size`, until it stops answering.
async fn capture(
    adb: PathBuf,
    id: DeviceId,
    max_side: u32,
    screen: watch::Sender<Screen>,
    size: watch::Sender<Option<(u32, u32)>>,
) {
    let mut last = Vec::new();
    let mut next_shown = tokio::time::Instant::now();

    let reason = loop {
        let next_frame = tokio::time::Instant::now() + FRAME_GAP;
        // Asked before every frame, so a foldable's view changes screens
        // on the next frame after it opens or closes.
        let lit = call(&adb, &id, &["shell", "dumpsys", "display"], CALL_TIMEOUT)
            .await
            .ok()
            .and_then(|out| lit_screen(&String::from_utf8_lossy(&out)));
        let mut args = vec!["exec-out", "screencap", "-p"];

        if let Some(display) = &lit {
            args.extend(["-d", display]);
        }

        let png = match call(&adb, &id, &args, CALL_TIMEOUT).await {
            Ok(png) => png,
            Err(e) => break format!("The phone stopped answering ({e:#})."),
        };

        if png != last {
            last.clone_from(&png);

            match tokio::task::spawn_blocking(move || frame(&png, max_side)).await {
                Ok(Ok((full, frame))) => {
                    tokio::time::sleep_until(next_shown).await;
                    size.send_replace(Some(full));
                    screen.send_replace(Screen::Frame(frame));
                    next_shown = tokio::time::Instant::now() + FRAME_GAP;
                }
                Ok(Err(e)) => log::warn!("a phone screenshot was not shown: {e:#}"),
                Err(e) => log::warn!("a phone screenshot was not shown: {e}"),
            }
        }

        tokio::time::sleep_until(next_frame).await;
    };

    screen.send_replace(Screen::Ended(reason));
}

/// The screen that is on, by the physical id `screencap -d` takes, from
/// `dumpsys display`; `None` for a phone with one screen, which needs none.
pub(super) fn lit_screen(dumped: &str) -> Option<String> {
    let mut screens = Vec::new();
    let mut id = None;

    for line in dumped.lines().map(str::trim) {
        if let Some(found) = line.strip_prefix("mPhysicalDisplayId=") {
            id = Some(found);
        } else if let Some(state) = line.strip_prefix("mState=")
            && let Some(id) = id.take()
        {
            screens.push((id, state == "ON"));
        }
    }

    if screens.len() < 2 {
        return None;
    }

    screens
        .into_iter()
        .find_map(|(id, on)| on.then(|| id.to_owned()))
}

/// A PNG screenshot as a frame no longer than `max_side`, with the
/// screenshot's own size. Anything `screencap` printed before the image,
/// such as a warning, is skipped.
pub(super) fn frame(png: &[u8], max_side: u32) -> Result<((u32, u32), Frame)> {
    let start = png
        .windows(PNG_SIGNATURE.len())
        .position(|window| window == PNG_SIGNATURE)
        .context("screencap gave no image")?;
    let image = image::load_from_memory_with_format(&png[start..], ImageFormat::Png)?;
    let full = (image.width(), image.height());
    let image = if full.0.max(full.1) > max_side {
        image.resize(max_side, max_side, FilterType::Triangle)
    } else {
        image
    };
    let rgb = image.into_rgb8();
    let mut jpeg = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY))?;
    let frame = Frame {
        jpeg,
        width: rgb.width(),
        height: rgb.height(),
    };

    Ok((full, frame))
}

/// Input to an attached phone.
pub(super) struct Controls {
    adb: PathBuf,
    id: DeviceId,
    /// The screen's size in pixels as the last screenshot shows it.
    size: watch::Receiver<Option<(u32, u32)>>,
    /// Where and when the finger went down, until it lifts.
    down: Option<((f64, f64), Instant)>,
}

impl Controls {
    pub(super) async fn send(&mut self, input: Input) -> Result<()> {
        match input {
            Input::Touch { phase, x, y } => self.touch(phase, x, y).await,
            Input::Text { text } => {
                if text.len() > MAX_TEXT {
                    bail!("text over {MAX_TEXT} bytes");
                }

                self.shell(&typed(&text)).await
            }
            Input::Key { key } => self.shell(&press(key_code(key))).await,
            Input::Button {
                button: Button::Rotate,
            } => self.shell(ROTATE).await,
            Input::Button { button } => match button_code(button) {
                Some(code) => self.shell(&press(code)).await,
                None => Ok(()),
            },
            other => bail!("unknown input {other:?}"),
        }
    }

    /// Keeps where the finger went down and sends the whole gesture once it
    /// lifts: one `input` per gesture, since each takes a moment to start.
    async fn touch(&mut self, phase: TouchPhase, x: f32, y: f32) -> Result<()> {
        let (width, height) =
            watched::read(&self.size, |size| *size).context("the phone has shown no screen yet")?;
        let at = point(x, y, f64::from(width), f64::from(height));

        match phase {
            TouchPhase::Down => self.down = Some((at, Instant::now())),
            TouchPhase::Move => {}
            TouchPhase::Up => {
                if let Some((from, since)) = self.down.take() {
                    self.shell(&gesture(from, at, since.elapsed())).await?;
                }
            }
        }

        Ok(())
    }

    async fn shell(&self, command: &str) -> Result<()> {
        call(&self.adb, &self.id, &["shell", command], CALL_TIMEOUT).await?;
        Ok(())
    }
}

/// The `input` command for a finger from `from` to `to` held for `held`: a
/// tap, a long press or a swipe.
pub(super) fn gesture(from: (f64, f64), to: (f64, f64), held: Duration) -> String {
    let moved = (to.0 - from.0).hypot(to.1 - from.1);
    let to = if moved <= TAP_SLOP { from } else { to };
    // Points on a screen, which an i64 holds.
    let [x1, y1, x2, y2] = [from.0, from.1, to.0, to.1].map(|at| at.round() as i64);

    if moved <= TAP_SLOP && held < LONG_PRESS {
        format!("input tap {x1} {y1}")
    } else {
        format!("input swipe {x1} {y1} {x2} {y2} {}", held.as_millis())
    }
}

/// The shell command that types `text`, a line at a time with Enter
/// between. `input text` reads `%s` as a space; the rest is quoted.
pub(super) fn typed(text: &str) -> String {
    let mut commands = Vec::new();

    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            commands.push(press(key_code(Key::Return)));
        }

        let line = line.trim_end_matches('\r');

        if !line.is_empty() {
            let quoted = line.replace(' ', "%s").replace('\'', r"'\''");
            commands.push(format!("input text '{quoted}'"));
        }
    }

    commands.join("; ")
}

fn press(code: &str) -> String {
    format!("input keyevent {code}")
}

fn key_code(key: Key) -> &'static str {
    match key {
        Key::Return => "KEYCODE_ENTER",
        Key::Delete => "KEYCODE_DEL",
        Key::Tab => "KEYCODE_TAB",
        Key::Escape => "KEYCODE_ESCAPE",
        Key::Left => "KEYCODE_DPAD_LEFT",
        Key::Right => "KEYCODE_DPAD_RIGHT",
        Key::Up => "KEYCODE_DPAD_UP",
        Key::Down => "KEYCODE_DPAD_DOWN",
    }
}

/// The key code of a button, `None` for one a key cannot press.
fn button_code(button: Button) -> Option<&'static str> {
    match button {
        Button::Home => Some("KEYCODE_HOME"),
        Button::Back => Some("KEYCODE_BACK"),
        Button::AppSwitch => Some("KEYCODE_APP_SWITCH"),
        Button::Power => Some("KEYCODE_POWER"),
        Button::VolumeUp => Some("KEYCODE_VOLUME_UP"),
        Button::VolumeDown => Some("KEYCODE_VOLUME_DOWN"),
        Button::Rotate => None,
    }
}

#[cfg(test)]
#[path = "../../tests/run_device_phone.rs"]
mod tests;
