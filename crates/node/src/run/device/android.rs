//! Android emulators: the AVDs `emulator -list-avds` names, the running ones
//! from the discovery files each emulator writes, and each one's gRPC
//! `EmulatorController` for its screen and input.

use super::{Controls as AnyControls, Live, Screen, Turn, Turned, VideoFeed, follow, point};
use crate::{lock, machine::env, run::process};
use anyhow::{Context, Result, bail};
use beep_proto::node::device::{
    Button, Device, DeviceId, DeviceState, Frame, Input, Key, MAX_TEXT, Platform, TouchPhase,
};
use futures::future::join_all;
use image::{ExtendedColorType, codecs::jpeg::JpegEncoder};
use ini::{Ini, ParseOption};
use proto::{
    EmulatorStatus, Image, ImageFormat, KeyboardEvent, ParameterValue, PhysicalModelValue, Touch,
    TouchEvent, VmRunState, emulator_controller_client::EmulatorControllerClient,
    image_format::ImgFormat, keyboard_event::KeyEventType, physical_model_value::PhysicalType,
    rotation::SkinRotation, vm_run_state::RunState,
};
use secrecy::{ExposeSecret, SecretString};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::watch, time::timeout};
use tokio_util::task::AbortOnDropHandle;
use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue},
    service::{Interceptor, interceptor::InterceptedService},
    transport::{Channel, Endpoint},
};

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod proto {
    tonic::include_proto!("android.emulation.control");
}

/// How long a started emulator counts as booting before it has written its
/// discovery file.
const BOOT_WAIT: Duration = Duration::from_secs(180);
/// How long an emulator that cannot start has to say so.
const EARLY_EXIT: Duration = Duration::from_secs(3);
/// How long the emulator's gRPC service has to answer.
const CALL_TIMEOUT: Duration = Duration::from_secs(3);
/// The largest message the emulator sends: a full screen of RGB pixels.
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
const JPEG_QUALITY: u8 = 80;

/// AVDs this node started that have not shown up running yet, by name.
static BOOTING: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Mutex::default);

type Client = EmulatorControllerClient<InterceptedService<Channel, Bearer>>;

/// A running emulator, as its discovery file describes it.
#[derive(Debug)]
pub(super) struct Running {
    avd: String,
    port: u16,
    token: SecretString,
    /// The AVD's folder, with its `config.ini`.
    dir: Option<PathBuf>,
    /// The console port adb names it by, as in `emulator-5554`.
    console: Option<u16>,
}

/// Every AVD here, running or not.
pub(super) async fn list() -> Vec<Device> {
    let (names, running) = tokio::join!(env::avds(), running());
    let mut states: BTreeMap<String, (DeviceState, Option<PathBuf>)> = names
        .into_iter()
        .map(|name| {
            let state = if booting(&name) {
                DeviceState::Booting
            } else {
                DeviceState::Off
            };
            (name, (state, None))
        })
        .collect();

    for (emulator, booted) in running {
        let state = if booted {
            DeviceState::On
        } else {
            DeviceState::Booting
        };
        states.insert(emulator.avd, (state, emulator.dir));
    }

    let avds = avd_home();
    states
        .into_iter()
        .map(|(name, (state, dir))| {
            let dir = dir.or_else(|| avds.as_ref().map(|home| home.join(format!("{name}.avd"))));
            let config = dir
                .and_then(|dir| std::fs::read_to_string(dir.join("config.ini")).ok())
                .and_then(|text| AvdConfig::parse(&text));
            device(name, state, config)
        })
        .collect()
}

fn device(name: String, state: DeviceState, config: Option<AvdConfig>) -> Device {
    let (shown, os) = match config {
        Some(config) => (config.name, config.os),
        None => (None, None),
    };
    Device {
        name: shown.unwrap_or_else(|| name.replace('_', " ")),
        id: DeviceId(name),
        platform: Platform::Android,
        state,
        os,
        phone: false,
    }
}

/// What an AVD's `config.ini` says of it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct AvdConfig {
    name: Option<String>,
    /// Such as "Android 16".
    os: Option<String>,
}

impl AvdConfig {
    fn parse(text: &str) -> Option<Self> {
        let ini = read_ini(text)?;
        let section = ini.general_section();
        let name = section.get("avd.ini.displayname").map(str::to_owned);
        let os = section
            .get("target")
            .and_then(|target| target.strip_prefix("android-"))
            .and_then(android_version);
        Some(Self { name, os })
    }
}

/// The Android release an API level such as "36" or "36.1" is.
fn android_version(api: &str) -> Option<String> {
    let level: u32 = api.split('.').next()?.parse().ok()?;
    let release = match level {
        33.. => (level - 20).to_string(),
        32 => "12L".to_owned(),
        31 => "12".to_owned(),
        30 => "11".to_owned(),
        29 => "10".to_owned(),
        _ => return Some(format!("API {level}")),
    };
    Some(format!("Android {release}"))
}

/// Where the AVDs live, as the emulator looks for them.
fn avd_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("ANDROID_AVD_HOME") {
        return Some(PathBuf::from(home));
    }

    if let Some(user) = std::env::var_os("ANDROID_USER_HOME") {
        return Some(PathBuf::from(user).join("avd"));
    }

    dirs::home_dir().map(|home| home.join(".android").join("avd"))
}

fn booting(name: &str) -> bool {
    lock(&BOOTING)
        .get(name)
        .is_some_and(|started| started.elapsed() < BOOT_WAIT)
}

fn read_ini(text: &str) -> Option<Ini> {
    let options = ParseOption {
        enabled_quote: false,
        enabled_escape: false,
        ..ParseOption::default()
    };
    Ini::load_from_str_opt(text, options).ok()
}

/// The emulators running now, each with whether it finished booting. One
/// whose service does not answer is gone, whatever its file says.
async fn running() -> Vec<(Running, bool)> {
    let found = discovered().await;
    let checked = join_all(found.into_iter().map(|emulator| async move {
        let booted = status(&emulator).await.ok()?.booted;
        Some((emulator, booted))
    }))
    .await;

    for (emulator, _) in checked.iter().flatten() {
        lock(&BOOTING).remove(&emulator.avd);
    }

    checked.into_iter().flatten().collect()
}

async fn status(emulator: &Running) -> Result<EmulatorStatus> {
    let mut client = client(emulator).await?;
    let status = timeout(CALL_TIMEOUT, client.get_status(()))
        .await
        .context("the emulator did not answer")??;
    Ok(status.into_inner())
}

/// What the emulators' discovery files describe.
async fn discovered() -> Vec<Running> {
    let mut found = Vec::new();

    for dir in discovery_dirs() {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if !(name.starts_with("pid_") && name.ends_with(".ini")) {
                continue;
            }

            if let Ok(text) = tokio::fs::read_to_string(entry.path()).await
                && let Some(running) = parse_discovery(&text)
            {
                found.push(running);
            }
        }
    }

    found
}

/// A discovery file's AVD, gRPC port and token; `None` for an emulator
/// started without a token, which Beep does not reach.
pub(super) fn parse_discovery(text: &str) -> Option<Running> {
    let ini = read_ini(text)?;
    let section = ini.general_section();
    Some(Running {
        avd: section.get("avd.name")?.to_owned(),
        port: section.get("grpc.port")?.parse().ok()?,
        token: SecretString::from(section.get("grpc.token")?),
        dir: section.get("avd.dir").map(PathBuf::from),
        console: section
            .get("port.serial")
            .and_then(|port| port.parse().ok()),
    })
}

/// The folders running emulators write their discovery files to.
fn discovery_dirs() -> Vec<PathBuf> {
    let running = |base: PathBuf| base.join("avd").join("running");
    #[cfg(target_os = "macos")]
    let bases = dirs::cache_dir().map(|caches| caches.join("TemporaryItems"));
    #[cfg(windows)]
    let bases = dirs::data_local_dir().map(|local| local.join("Temp"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let bases = dirs::runtime_dir().into_iter().chain(
        std::env::var_os("USER")
            .map(|user| std::env::temp_dir().join(format!("android-{}", user.to_string_lossy()))),
    );
    bases.into_iter().map(running).collect()
}

/// The adb serial of the running emulator of AVD `id`.
pub(super) async fn serial(id: &DeviceId) -> Result<DeviceId> {
    let found = running().await.into_iter().find(|(e, _)| e.avd == id.0);
    let (emulator, _) = found.with_context(|| format!("{id} is not running."))?;
    let console = emulator.console.context("the emulator gave no adb port")?;

    Ok(DeviceId(format!("emulator-{console}")))
}

/// Starts AVD `id` without a window, to outlive the stream and the node.
pub(super) async fn boot(id: &DeviceId) -> Result<()> {
    if running()
        .await
        .iter()
        .any(|(emulator, _)| emulator.avd == id.0)
    {
        return Ok(());
    }

    let emulator = env::emulator().context("No Android emulator on this machine.")?;
    let mut command = process::command(&emulator);
    process::own_group(&mut command)
        .args(["-avd", &id.0, "-no-window", "-grpc-use-token"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false);
    let started = command.spawn();
    process::launched(&emulator, &started);
    let mut child = started.context("could not start the emulator")?;
    lock(&BOOTING).insert(id.0.clone(), Instant::now());

    if let Ok(exited) = timeout(EARLY_EXIT, child.wait()).await {
        lock(&BOOTING).remove(&id.0);
        let status = exited.context("could not start the emulator")?;
        bail!("The emulator for {id} stopped at once ({status}).");
    }

    // Reaped when it exits, whenever that is.
    let name = id.0.clone();
    tokio::spawn(async move {
        let _ = child.wait().await;
        lock(&BOOTING).remove(&name);
    });
    Ok(())
}

pub(super) async fn shutdown(id: &DeviceId) -> Result<()> {
    lock(&BOOTING).remove(&id.0);
    let found = running().await.into_iter().find(|(e, _)| e.avd == id.0);
    let Some((emulator, _)) = found else {
        return Ok(());
    };
    let mut client = client(&emulator).await?;
    let state = VmRunState {
        state: RunState::Shutdown.into(),
    };
    timeout(CALL_TIMEOUT, client.set_vm_state(state))
        .await
        .context("the emulator did not answer")??;
    Ok(())
}

/// Follows AVD `id`'s screen, waiting for it while it boots.
pub(super) async fn attach(id: &DeviceId, max_side: u32) -> Result<Live> {
    let controls = controls(id).await?;
    let mut client = controls.client.clone();
    let (width, height) = (controls.width, controls.height);
    let side = max_side.min(width.max(height));
    let format = ImageFormat {
        format: ImgFormat::Rgb888.into(),
        width: side,
        height: side,
        ..ImageFormat::default()
    };
    let stream = client.stream_screenshot(format).await?.into_inner();
    let (screen, shown) = watch::channel(Screen::Starting);
    let turn = controls.turned.clone();
    let mut last = Vec::new();
    let show = async move |image: Image| -> Result<Option<Frame>> {
        let Some(format) = image.format else {
            return Ok(None);
        };

        if image.image.is_empty() || image.image == last {
            return Ok(None);
        }

        let rotation = format.rotation.map(|rotation| rotation.rotation());
        turn.set(rotation.map(Turn::from).unwrap_or_default());
        last.clone_from(&image.image);
        encode(image.image, format.width, format.height)
            .await
            .map(Some)
    };
    let work = tokio::spawn(follow(stream, screen, show, "The emulator stopped."));
    Ok(Live {
        screen: shown,
        controls: AnyControls::Android(Box::new(controls)),
        _work: AbortOnDropHandle::new(work),
    })
}

/// Uses Android's native encoder for both emulators and physical phones.
pub(super) async fn video(id: &DeviceId, max_side: u32, phone: bool) -> Result<VideoFeed> {
    use super::phone as phones;
    use beep_proto::node::device_video::Reply;
    use tokio::sync::{mpsc, watch};

    let (controls, size, turned, serial) = if phone {
        let phones::VideoControls { controls, size } = phones::video_controls(id)?;
        (
            AnyControls::Phone(Box::new(controls)),
            size,
            None,
            id.clone(),
        )
    } else {
        let controls = controls(id).await?;
        let turned = controls.turned.clone();
        let (size, _) = watch::channel(None);
        (
            AnyControls::Android(Box::new(controls)),
            size,
            Some(turned),
            serial(id).await?,
        )
    };
    let adb = env::adb().context(super::adb::NO_ADB)?;
    let (packets, received) = mpsc::channel(2);
    let work = tokio::spawn(async move {
        let result = record(&adb, &serial, max_side, &size, turned.as_ref(), &packets).await;
        if let Err(error) = result {
            let _ = packets
                .send(Reply::Ended {
                    reason: format!("{error:#}"),
                })
                .await;
        }
    });
    // Rotation is already encoded into Android's projection. Emulator touch
    // coordinates still use the physical display's upright coordinate system.
    Ok(VideoFeed {
        packets: received,
        controls,
        work: AbortOnDropHandle::new(work),
    })
}

async fn controls(id: &DeviceId) -> Result<Controls> {
    let emulator = wait_running(id).await?;
    let mut client = client(&emulator).await?;
    let status = timeout(CALL_TIMEOUT, client.get_status(()))
        .await
        .context("the emulator did not answer")??
        .into_inner();
    let (width, height) = lcd_size(&status).context("the emulator gave no screen size")?;
    Ok(Controls {
        client,
        width,
        height,
        turned: Turned::default(),
    })
}

async fn record(
    adb: &Path,
    id: &DeviceId,
    max_side: u32,
    size: &watch::Sender<Option<(u32, u32)>>,
    turned: Option<&Turned>,
    packets: &tokio::sync::mpsc::Sender<beep_proto::node::device_video::Reply>,
) -> Result<()> {
    use beep_proto::node::device_video::{Reply, Rotation};
    use tokio::io::AsyncReadExt;

    let mut current = read_video_geometry(adb, id).await?;
    loop {
        size.send_replace(Some((current.width, current.height)));
        if let Some(turned) = turned {
            turned.set(current.turn);
        }
        let (width, height) = video_size(current.width, current.height, max_side)?;
        let dimensions = format!("{width}x{height}");
        let mut command = process::command(adb);
        command
            .args([
                "-s",
                &id.0,
                "exec-out",
                "screenrecord",
                "--output-format=h264",
                "--bit-rate",
                "8000000",
                "--size",
                &dimensions,
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .context("could not start Android video capture")?;
        let mut output = child.stdout.take().context("Android video has no output")?;
        let mut bytes = vec![0; beep_proto::node::device_video::MAX_CHUNK];
        let mut started = false;
        let checking = next_video_geometry(adb, id);
        tokio::pin!(checking);
        loop {
            tokio::select! {
                read = output.read(&mut bytes) => {
                    let count = read.context("could not read Android video")?;
                    if count == 0 {
                        let status = child.wait().await?;
                        if !status.success() || !started {
                            bail!("Android video capture stopped ({status})");
                        }
                        // screenrecord's duration limit starts a fresh encoder, never
                        // a delta stream without its configuration and first keyframe.
                        break;
                    }
                    if !started {
                        packets.send(Reply::Started { fps: beep_proto::node::device_video::FPS }).await?;
                        started = true;
                    }
                    packets.send(Reply::Data { data: bytes[..count].to_vec(), rotation: Rotation::Upright }).await?;
                }
                next = &mut checking => {
                    let next = next?;
                    if next != current {
                        current = next;
                        break;
                    }
                    checking.set(next_video_geometry(adb, id));
                }
                _ = packets.closed() => return Ok(()),
            }
        }
        // Reap only the recorder this attachment started.
        if child.try_wait()?.is_none() {
            child.kill().await?;
        }
    }
}

async fn next_video_geometry(adb: &Path, id: &DeviceId) -> Result<VideoGeometry> {
    tokio::time::sleep(Duration::from_millis(500)).await;
    read_video_geometry(adb, id).await
}

async fn read_video_geometry(adb: &Path, id: &DeviceId) -> Result<VideoGeometry> {
    let printed = super::adb::call(adb, id, &["shell", "dumpsys", "display"], CALL_TIMEOUT).await?;
    video_geometry(&String::from_utf8_lossy(&printed))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VideoGeometry {
    width: u32,
    height: u32,
    turn: Turn,
}

fn video_geometry(printed: &str) -> Result<VideoGeometry> {
    let pattern = regex::Regex::new(r"real (\d+) x (\d+).*?rotation ([0-3])")?;
    let line = printed
        .lines()
        .find(|line| line.contains("mOverrideDisplayInfo=") && line.contains("displayId 0"))
        .or_else(|| {
            printed
                .lines()
                .find(|line| line.contains("DisplayInfo{") && line.contains("displayId 0"))
        })
        .context("Android gave no default display geometry")?;
    let info = pattern
        .captures(line)
        .context("Android gave no video dimensions")?;
    let number = |index| -> Result<u32> {
        Ok(info
            .get(index)
            .context("missing display value")?
            .as_str()
            .parse()?)
    };
    let width = number(1)?;
    let height = number(2)?;
    if width == 0 || height == 0 {
        bail!("Android gave an empty display");
    }
    let turn = match number(3)? {
        0 => Turn::Upright,
        1 => Turn::Left,
        2 => Turn::UpsideDown,
        _ => Turn::Right,
    };
    Ok(VideoGeometry {
        width,
        height,
        turn,
    })
}

fn video_size(width: u32, height: u32, max_side: u32) -> Result<(u32, u32)> {
    let longest = width.max(height);
    if longest == 0 {
        bail!("the display is empty");
    }
    let scale = u64::from(max_side.min(longest));
    let even = |side| -> Result<u32> {
        Ok((u32::try_from(u64::from(side) * scale / u64::from(longest))? / 2 * 2).max(2))
    };
    Ok((even(width)?, even(height)?))
}

/// The emulator running AVD `id`, waited for while this node boots it.
async fn wait_running(id: &DeviceId) -> Result<Running> {
    loop {
        let found = running().await.into_iter().find(|(e, _)| e.avd == id.0);

        if let Some((emulator, _)) = found {
            return Ok(emulator);
        }

        if !booting(&id.0) {
            bail!("{id} is not running.");
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// The screen's size in pixels, upright, from the emulator's hardware.
fn lcd_size(status: &EmulatorStatus) -> Option<(u32, u32)> {
    let entries = &status.hardware_config.as_ref()?.entry;
    let value = |key: &str| {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.value.parse().ok())
    };
    Some((value("hw.lcd.width")?, value("hw.lcd.height")?))
}

/// RGB pixels as a JPEG, off the async workers.
async fn encode(rgb: Vec<u8>, width: u32, height: u32) -> Result<Frame> {
    tokio::task::spawn_blocking(move || {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY).encode(
            &rgb,
            width,
            height,
            ExtendedColorType::Rgb8,
        )?;
        Ok(Frame {
            jpeg,
            width,
            height,
        })
    })
    .await?
}

async fn client(emulator: &Running) -> Result<Client> {
    let mut token: MetadataValue<Ascii> =
        format!("Bearer {}", emulator.token.expose_secret()).parse()?;
    token.set_sensitive(true);
    let channel = Endpoint::from_shared(format!("http://127.0.0.1:{}", emulator.port))?
        .connect_timeout(CALL_TIMEOUT)
        .connect()
        .await
        .with_context(|| format!("could not reach the emulator for {}", emulator.avd))?;
    Ok(
        EmulatorControllerClient::with_interceptor(channel, Bearer(token))
            .max_decoding_message_size(MAX_MESSAGE),
    )
}

/// Adds the emulator's token to each call.
#[derive(Clone)]
struct Bearer(MetadataValue<Ascii>);

impl Interceptor for Bearer {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        request
            .metadata_mut()
            .insert("authorization", self.0.clone());
        Ok(request)
    }
}

/// Input to an attached emulator.
pub(super) struct Controls {
    client: Client,
    /// The screen's size in pixels, upright.
    width: u32,
    height: u32,
    turned: Turned,
}

impl Controls {
    pub(super) async fn send(&mut self, input: Input) -> Result<()> {
        match input {
            Input::Touch { phase, x, y } => {
                let (x, y) = self.turned.get().upright(x, y);
                let (width, height) = (self.width, self.height);
                let (x, y) = point(x, y, f64::from(width), f64::from(height));
                let touch = Touch {
                    x: pixel(x, width),
                    y: pixel(y, height),
                    identifier: 0,
                    pressure: if phase == TouchPhase::Up { 0 } else { 1 },
                    ..Touch::default()
                };
                let event = TouchEvent {
                    touches: vec![touch],
                    display: 0,
                };
                self.client.send_touch(event).await?;
            }
            Input::Text { text } => {
                if text.len() > MAX_TEXT {
                    bail!("text over {MAX_TEXT} bytes");
                }

                let event = KeyboardEvent {
                    event_type: KeyEventType::Keypress.into(),
                    text,
                    ..KeyboardEvent::default()
                };
                self.client.send_key(event).await?;
            }
            Input::Key { key } => self.press(key_name(key)).await?,
            Input::Button {
                button: Button::Rotate,
            } => self.rotate().await?,
            Input::Button { button } => {
                if let Some(key) = button_key(button) {
                    self.press(key).await?;
                }
            }
            other => bail!("unknown input {other:?}"),
        }

        Ok(())
    }

    async fn press(&mut self, key: &str) -> Result<()> {
        let event = KeyboardEvent {
            event_type: KeyEventType::Keypress.into(),
            key: key.to_owned(),
            ..KeyboardEvent::default()
        };
        self.client.send_key(event).await?;
        Ok(())
    }

    /// Turns the device a quarter turn to the left, as the emulator's own
    /// rotate button does.
    async fn rotate(&mut self) -> Result<()> {
        let asked = PhysicalModelValue {
            target: PhysicalType::Rotation.into(),
            ..PhysicalModelValue::default()
        };
        let now = self.client.get_physical_model(asked).await?.into_inner();
        let mut angles = now.value.map(|value| value.data).unwrap_or_default();
        angles.resize(3, 0.0);
        angles[2] = quarter_turn(angles[2]);
        let turned = PhysicalModelValue {
            target: PhysicalType::Rotation.into(),
            value: Some(ParameterValue { data: angles }),
            ..PhysicalModelValue::default()
        };
        self.client.set_physical_model(turned).await?;
        Ok(())
    }
}

impl From<SkinRotation> for Turn {
    fn from(rotation: SkinRotation) -> Self {
        match rotation {
            SkinRotation::Portrait => Self::Upright,
            SkinRotation::Landscape => Self::Left,
            SkinRotation::ReversePortrait => Self::UpsideDown,
            SkinRotation::ReverseLandscape => Self::Right,
        }
    }
}

/// The rotation around the screen's axis a quarter turn on from `degrees`,
/// within -180 to 180.
fn quarter_turn(degrees: f32) -> f32 {
    let turned = (degrees + 90.0).rem_euclid(360.0);

    if turned > 180.0 {
        turned - 360.0
    } else {
        turned
    }
}

/// A pixel position from a point on a screen `side` pixels long.
fn pixel(at: f64, side: u32) -> i32 {
    let last = f64::from(side.saturating_sub(1));
    // Within 0..side, which an i32 holds.
    at.round().clamp(0.0, last) as i32
}

/// The W3C key value the emulator takes for `key`.
fn key_name(key: Key) -> &'static str {
    match key {
        Key::Return => "Enter",
        Key::Delete => "Backspace",
        Key::Tab => "Tab",
        Key::Escape => "Escape",
        Key::Left => "ArrowLeft",
        Key::Right => "ArrowRight",
        Key::Up => "ArrowUp",
        Key::Down => "ArrowDown",
    }
}

/// The W3C key value of a button, `None` for one the emulator has no key
/// for.
fn button_key(button: Button) -> Option<&'static str> {
    match button {
        Button::Home => Some("GoHome"),
        Button::Back => Some("GoBack"),
        Button::AppSwitch => Some("AppSwitch"),
        Button::Power => Some("Power"),
        Button::VolumeUp => Some("AudioVolumeUp"),
        Button::VolumeDown => Some("AudioVolumeDown"),
        Button::Rotate => None,
    }
}

#[cfg(test)]
#[path = "../../tests/run_device_android.rs"]
mod tests;
