//! iOS simulators, on macOS: listed, started and stopped with `simctl`, and
//! shown through an `idb_companion` the stream starts for itself, on a Unix
//! socket only this node uses.

use super::screen::{Live, Screen, Turn, Turned, VideoFeed, follow, point};
use crate::{lock, process};
use anyhow::{Context, Result, bail};
use beep_proto::node::device::{
    Button, Device, DeviceId, DeviceState, Element, Frame, Input, Key, MAX_TEXT, Platform,
    TouchPhase,
};
use hyper_util::rt::TokioIo;
use image::{
    ImageDecoder, ImageFormat,
    codecs::jpeg::{JpegDecoder, JpegEncoder},
};
use proto::{
    AccessibilityInfoRequest, GetOrientationRequest, HidEvent, Point, TargetDescriptionRequest,
    VideoStreamRequest,
    companion_service_client::CompanionServiceClient,
    get_orientation_response::Orientation,
    hid_event::{
        Event, HidButton, HidButtonType, HidDirection, HidKey, HidOrientation, HidOrientationType,
        HidPress, HidPressAction, HidTouch, hid_press_action::Action,
    },
    payload::Source,
    video_stream_request::{Control, Format, Start},
    video_stream_response::Output,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use tokio::{
    net::UnixStream,
    process::Child,
    sync::{mpsc, watch},
    time::timeout,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::task::AbortOnDropHandle;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod proto {
    tonic::include_proto!("idb");
}

/// How long a simulator has to finish booting before attaching gives up.
const BOOT_WAIT: Duration = Duration::from_secs(180);
/// How long idb_companion has to open its socket.
const COMPANION_START: Duration = Duration::from_secs(20);
const FPS: u64 = 30;
const JPEG_QUALITY: f64 = 0.8;
/// The quality of a frame turned here, out of 100.
const JPEG_REENCODED: u8 = 80;
/// USB HID usage of the left shift key.
const SHIFT: u64 = 225;
const NO_COMPANION: &str =
    "idb_companion is not installed: install it with `brew install meta/fb/idb-companion`.";

type Client = CompanionServiceClient<Channel>;

/// How this node last turned each simulator, by UDID, for those that do
/// not say.
static TURNS: LazyLock<Mutex<HashMap<String, Turn>>> = LazyLock::new(Mutex::default);

/// Every available iPhone and iPad simulator.
pub(super) async fn list() -> Vec<Device> {
    let text = match simctl(&["list", "devices", "available", "--json"]).await {
        Ok(text) => text,
        Err(e) => {
            log::debug!("no iOS simulators: {e:#}");
            return Vec::new();
        }
    };

    match parse_simulators(&text) {
        Ok(devices) => devices,
        Err(e) => {
            log::warn!("could not read the iOS simulators: {e:#}");
            Vec::new()
        }
    }
}

/// `xcrun simctl list devices --json`, by runtime.
#[derive(Deserialize)]
struct SimctlList {
    devices: BTreeMap<String, Vec<SimctlDevice>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimctlDevice {
    udid: String,
    name: String,
    state: String,
    #[serde(default)]
    device_type_identifier: String,
    #[serde(default = "available")]
    is_available: bool,
}

fn available() -> bool {
    true
}

/// The iPhones and iPads `simctl`'s JSON lists, with their iOS version.
pub(super) fn parse_simulators(text: &str) -> Result<Vec<Device>> {
    let list: SimctlList = serde_json::from_str(text)?;
    let devices = list
        .devices
        .into_iter()
        .filter_map(|(runtime, devices)| Some((ios_version(&runtime)?, devices)))
        .flat_map(|(os, devices)| {
            devices
                .into_iter()
                .filter(|device| device.is_available && is_phone_or_pad(device))
                .map(move |device| Device {
                    id: DeviceId(device.udid),
                    name: device.name,
                    platform: Platform::Ios,
                    state: state(&device.state),
                    os: Some(os.clone()),
                    phone: false,
                })
        })
        .collect();
    Ok(devices)
}

fn is_phone_or_pad(device: &SimctlDevice) -> bool {
    let kind = &device.device_type_identifier;
    kind.contains("iPhone") || kind.contains("iPad")
}

/// "iOS 26.5" from a runtime such as
/// `com.apple.CoreSimulator.SimRuntime.iOS-26-5`; `None` for other systems.
fn ios_version(runtime: &str) -> Option<String> {
    let version = runtime.rsplit('.').next()?.strip_prefix("iOS-")?;
    Some(format!("iOS {}", version.replace('-', ".")))
}

fn state(simctl: &str) -> DeviceState {
    match simctl {
        "Booted" => DeviceState::On,
        "Booting" => DeviceState::Booting,
        _ => DeviceState::Off,
    }
}

pub(super) async fn boot(id: &DeviceId) -> Result<()> {
    match simctl(&["boot", &id.0]).await {
        Ok(_) => Ok(()),
        // Booting one already booted changes nothing.
        Err(e) if format!("{e:#}").contains("current state: Booted") => Ok(()),
        Err(e) => Err(e),
    }
}

pub(super) async fn shutdown(id: &DeviceId) -> Result<()> {
    match simctl(&["shutdown", &id.0]).await {
        Ok(_) => Ok(()),
        Err(e) if format!("{e:#}").contains("current state: Shutdown") => Ok(()),
        Err(e) => Err(e),
    }
}

/// What `xcrun simctl args` printed; an error with what it said when it
/// failed.
/// What simulator `id`'s screen shows, through an idb_companion of this
/// call's own.
pub(super) async fn ui(id: &DeviceId) -> Result<Vec<Element>> {
    let companion = Companion::start(&companion()?, id).await?;
    let mut client = CompanionServiceClient::new(companion.connect().await?);
    let described = client
        .describe(TargetDescriptionRequest::default())
        .await?
        .into_inner();
    let screen = described
        .target_description
        .and_then(|target| target.screen_dimensions)
        .context("idb_companion gave no screen size")?;
    let info = client
        .accessibility_info(AccessibilityInfoRequest::default())
        .await?
        .into_inner();

    accessible(
        &info.json,
        screen.width_points as f32,
        screen.height_points as f32,
    )
}

/// The elements of idb's accessibility list that say something or have an
/// id, placed as fractions of a `width` by `height` point screen.
pub(super) fn accessible(json: &str, width: f32, height: f32) -> Result<Vec<Element>> {
    #[derive(Deserialize)]
    struct Item {
        #[serde(rename = "AXLabel")]
        label: Option<String>,
        #[serde(rename = "AXValue")]
        value: Option<String>,
        #[serde(rename = "AXUniqueId")]
        id: Option<String>,
        #[serde(rename = "type", default)]
        kind: String,
        frame: Bounds,
    }
    #[derive(Deserialize)]
    struct Bounds {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    }

    let items: Vec<Item> = serde_json::from_str(json).context("idb's screen did not read")?;
    let (width, height) = (width.max(1.0), height.max(1.0));

    Ok(items
        .into_iter()
        .filter_map(|item| {
            let label = item
                .label
                .filter(|label| !label.is_empty())
                .or(item.value)
                .unwrap_or_default();
            let id = item.id.unwrap_or_default();

            if (label.is_empty() && id.is_empty()) || item.frame.width <= 0.0 {
                return None;
            }

            Some(Element {
                label,
                kind: item.kind,
                id,
                x: item.frame.x / width,
                y: item.frame.y / height,
                width: item.frame.width / width,
                height: item.frame.height / height,
            })
        })
        .collect())
}

/// Installs the `.app` at `path` on simulator `id`.
pub(super) async fn install(id: &DeviceId, path: &Path) -> Result<()> {
    let path = path.to_str().context("the path is not text")?;
    simctl(&["install", &id.0, path]).await?;

    Ok(())
}

/// Starts app `target` by its bundle id, or opens it when it is a URL.
pub(super) async fn launch(id: &DeviceId, target: &str) -> Result<()> {
    let verb = if target.contains("://") {
        "openurl"
    } else {
        "launch"
    };
    simctl(&[verb, &id.0, target]).await?;

    Ok(())
}

/// idb_companion, on `PATH` or where Homebrew and pip put it.
fn companion() -> Result<PathBuf> {
    which::which("idb_companion")
        .ok()
        .or_else(|| {
            [
                "/opt/homebrew/bin/idb_companion",
                "/usr/local/bin/idb_companion",
            ]
            .into_iter()
            .map(PathBuf::from)
            .chain(dirs::home_dir().map(|home| home.join(".local/bin/idb_companion")))
            .find(|path| path.is_file())
        })
        .context(NO_COMPANION)
}

/// Simulator `id`'s unified log of the last minutes, a line per entry.
pub(super) async fn logs(id: &DeviceId) -> Result<String> {
    simctl(&[
        "spawn", &id.0, "log", "show", "--last", "5m", "--style", "compact",
    ])
    .await
}

async fn simctl(args: &[&str]) -> Result<String> {
    let mut command = process::command("xcrun");
    command
        .arg("simctl")
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = command.output().await;
    process::finished("xcrun", &output);
    let output = output.context("could not run xcrun simctl")?;

    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        bail!("simctl {}: {}", args.join(" "), said.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Follows simulator `id`'s screen through an idb_companion of this
/// stream's own, once the simulator finished booting.
pub(super) async fn attach(id: &DeviceId, max_side: u32) -> Result<Live<Controls>> {
    let Capture {
        mut client,
        screen,
        encoder,
        companion,
    } = start_capture(id, max_side, Format::Mjpeg, false).await?;
    let Encoder { stream, requests } = encoder;
    let turned = Turned::default();
    turned.set(turn_of(&mut client, id).await);
    let turn = turned.clone();
    let mut last = (Vec::new(), Turn::Upright);
    let show = async move |response: proto::VideoStreamResponse| -> Result<Option<Frame>> {
        // Held while the stream runs: closing it would stop the video.
        let _requests = &requests;
        let Some(Output::Payload(payload)) = response.output else {
            return Ok(None);
        };
        let Some(Source::Data(jpeg)) = payload.source else {
            return Ok(None);
        };

        let turn = turn.get();

        if jpeg.is_empty() || (jpeg == last.0 && turn == last.1) {
            return Ok(None);
        }

        last = (jpeg.clone(), turn);
        tokio::task::spawn_blocking(move || turned_frame(jpeg, turn))
            .await?
            .map(Some)
    };
    let (sender, shown) = watch::channel(Screen::Starting);
    let work = tokio::spawn(follow(stream, sender, show, "The simulator stopped."));
    let controls = Controls {
        client,
        screen: watch::channel(screen).1,
        udid: id.clone(),
        turned,
        _companion: companion,
    };
    Ok(Live {
        screen: shown,
        controls,
        _work: AbortOnDropHandle::new(work),
    })
}

/// Passes the device's encoder output through without decoding or re-encoding.
pub(super) async fn video(
    id: &DeviceId,
    max_side: u32,
    phone: bool,
) -> Result<VideoFeed<Option<Controls>>> {
    use beep_proto::node::device_video::{self, Reply, Rotation};
    let Capture {
        mut client,
        mut screen,
        mut encoder,
        companion,
    } = start_capture(id, max_side, Format::H264, phone).await?;
    let turned = Turned::default();
    if !phone {
        turned.set(turn_of(&mut client, id).await);
    }
    let turn = turned.clone();
    let mut capture_client = client.clone();
    let id = id.clone();
    let (dimensions, input_screen) = watch::channel(screen);
    let (controls, companion) = if phone {
        (None, Some(companion))
    } else {
        (
            Some(Controls {
                client,
                screen: input_screen,
                udid: id.clone(),
                turned,
                _companion: companion,
            }),
            None,
        )
    };
    let (packets, received) = mpsc::channel(2);
    let work = tokio::spawn(async move {
        let _companion = companion;
        let result: Result<()> = async {
            let mut started = false;
            let mut encoded_turn = turn.get();
            let checking = next_video_geometry(capture_client.clone(), &id, phone);
            tokio::pin!(checking);
            loop {
                tokio::select! {
                    response = encoder.stream.message() => {
                        let Some(response) = response? else { break; };
                        let Some(Output::Payload(payload)) = response.output else { continue; };
                        let Some(Source::Data(data)) = payload.source else { continue; };
                        if data.is_empty() { continue; }
                        if !started {
                            dimensions.send_replace(screen);
                            packets.send(Reply::Started { fps: device_video::FPS }).await?;
                            started = true;
                        }
                        for chunk in data.chunks(device_video::MAX_CHUNK) {
                            packets.send(Reply::Data { data: chunk.to_vec(), rotation: Into::<Rotation>::into(encoded_turn) }).await?;
                        }
                    }
                    next = &mut checking => {
                        let (next, orientation) = next?;
                        if let Some(orientation) = orientation { turn.set(orientation); }
                        checking.set(next_video_geometry(capture_client.clone(), &id, phone));
                        if next == screen && encoded_turn == turn.get() { continue; }
                        // A new surface needs fresh SPS/PPS and an IDR frame. Stop
                        // the old encoder before opening a new byte sequence.
                        encoder.requests.send(VideoStreamRequest { control: Some(Control::Stop(proto::video_stream_request::Stop {})) }).await?;
                        timeout(Duration::from_secs(2), async {
                            while encoder.stream.message().await?.is_some() {}
                            Ok::<(), tonic::Status>(())
                        }).await??;
                        screen = next;
                        encoded_turn = turn.get();
                        encoder = capture_stream(&mut capture_client, screen, max_side, Format::H264).await?;
                        started = false;
                    }
                    _ = packets.closed() => break,
                }
            }
            Ok(())
        }.await;
        let reason = result.err().map_or_else(
            || "The device video stopped.".to_owned(),
            |error| format!("{error:#}"),
        );
        let _ = packets.send(Reply::Ended { reason }).await;
    });
    Ok(VideoFeed {
        packets: received,
        controls,
        work: AbortOnDropHandle::new(work),
    })
}

struct Capture {
    client: Client,
    screen: proto::ScreenDimensions,
    encoder: Encoder,
    companion: Companion,
}

struct Encoder {
    stream: tonic::Streaming<proto::VideoStreamResponse>,
    requests: mpsc::Sender<VideoStreamRequest>,
}

async fn start_capture(
    id: &DeviceId,
    max_side: u32,
    format: Format,
    phone: bool,
) -> Result<Capture> {
    let companion = companion()?;
    if !phone {
        timeout(BOOT_WAIT, simctl(&["bootstatus", &id.0]))
            .await
            .with_context(|| format!("{id} did not finish booting"))??;
    }
    let companion = Companion::start(&companion, id).await?;
    let mut client = CompanionServiceClient::new(companion.connect().await?);
    let screen = screen_dimensions(&mut client, phone).await?;
    let encoder = capture_stream(&mut client, screen, max_side, format).await?;
    Ok(Capture {
        client,
        screen,
        encoder,
        companion,
    })
}

async fn capture_stream(
    client: &mut Client,
    screen: proto::ScreenDimensions,
    max_side: u32,
    format: Format,
) -> Result<Encoder> {
    let longer = screen.width.max(screen.height).max(1);
    let scale = (f64::from(max_side) / longer as f64).min(1.0);
    let video = format == Format::H264;
    let start = VideoStreamRequest {
        control: Some(Control::Start(Start {
            fps: if video {
                u64::from(beep_proto::node::device_video::FPS)
            } else {
                FPS
            },
            format: format.into(),
            compression_quality: if video { 0.0 } else { JPEG_QUALITY },
            scale_factor: scale,
            avg_bitrate: if video { 8_000_000.0 } else { 0.0 },
            key_frame_rate: if video { 1.0 } else { 0.0 },
            ..Start::default()
        })),
    };
    let (requests, waiting) = mpsc::channel(1);
    requests.send(start).await?;
    let stream = client
        .video_stream(ReceiverStream::new(waiting))
        .await?
        .into_inner();
    Ok(Encoder { requests, stream })
}

async fn screen_dimensions(client: &mut Client, phone: bool) -> Result<proto::ScreenDimensions> {
    let described = timeout(
        Duration::from_secs(2),
        client.describe(TargetDescriptionRequest::default()),
    )
    .await??
    .into_inner();
    described
        .target_description
        .and_then(|target| target.screen_dimensions)
        .or_else(|| phone.then(proto::ScreenDimensions::default))
        .context("idb_companion gave no screen size")
}

async fn next_video_geometry(
    mut client: Client,
    id: &DeviceId,
    phone: bool,
) -> Result<(proto::ScreenDimensions, Option<Turn>)> {
    tokio::time::sleep(Duration::from_millis(500)).await;
    let screen = screen_dimensions(&mut client, phone).await?;
    let turn = if phone {
        None
    } else {
        Some(timeout(Duration::from_secs(2), turn_of(&mut client, id)).await?)
    };
    Ok((screen, turn))
}

/// The simulator's JPEG `jpeg` as a frame, turned as the device is.
fn turned_frame(jpeg: Vec<u8>, turn: Turn) -> Result<Frame> {
    if turn == Turn::Upright {
        let (width, height) = JpegDecoder::new(Cursor::new(&jpeg))?.dimensions();
        return Ok(Frame {
            jpeg,
            width,
            height,
        });
    }

    let image = image::load_from_memory_with_format(&jpeg, ImageFormat::Jpeg)?;
    let image = match turn {
        Turn::Upright => image,
        Turn::Left => image.rotate270(),
        Turn::UpsideDown => image.rotate180(),
        Turn::Right => image.rotate90(),
    };
    let mut jpeg = Vec::new();
    image.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, JPEG_REENCODED))?;
    Ok(Frame {
        jpeg,
        width: image.width(),
        height: image.height(),
    })
}

/// How simulator `id` is turned: as idb_companion says, else as this node
/// last turned it.
async fn turn_of(client: &mut Client, id: &DeviceId) -> Turn {
    let asked = client.get_orientation(GetOrientationRequest {}).await;

    match asked.map(|response| response.into_inner().orientation()) {
        Ok(Orientation::LandscapeLeft) => Turn::Right,
        Ok(Orientation::PortraitUpsideDown) => Turn::UpsideDown,
        Ok(Orientation::LandscapeRight) => Turn::Left,
        Ok(_) => Turn::Upright,
        // Simulators from Xcode 26 do not say.
        Err(_) => lock(&TURNS).get(&id.0).copied().unwrap_or_default(),
    }
}

/// An idb_companion for one simulator, killed with the stream that
/// started it, with its socket in a folder of its own.
struct Companion {
    child: Child,
    socket: PathBuf,
    _dir: tempfile::TempDir,
}

impl Companion {
    async fn start(program: &Path, id: &DeviceId) -> Result<Self> {
        let dir = tempfile::Builder::new().prefix("beep-idb").tempdir()?;
        let socket = dir.path().join("idb.sock");
        let mut command = process::command(program);
        command
            .arg("--udid")
            .arg(&id.0)
            .arg("--grpc-domain-sock")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let started = command.spawn();
        process::launched(program, &started);
        let mut child = started.context("could not start idb_companion")?;

        let opened = timeout(COMPANION_START, async {
            loop {
                if let Some(status) = child.try_wait()? {
                    bail!("idb_companion stopped at once ({status})");
                }

                if tokio::fs::try_exists(&socket).await.unwrap_or(false) {
                    return Ok(());
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        opened.context("idb_companion did not open its socket")??;

        Ok(Self {
            child,
            socket,
            _dir: dir,
        })
    }

    async fn connect(&self) -> Result<Channel> {
        let socket = self.socket.clone();
        // The address is not used: every connection goes to the socket.
        let channel =
            Endpoint::from_static("http://localhost")
                .connect_with_connector(service_fn(move |_: Uri| {
                    let socket = socket.clone();
                    async move {
                        Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(socket).await?))
                    }
                }))
                .await
                .context("could not reach idb_companion")?;
        Ok(channel)
    }
}

impl Drop for Companion {
    fn drop(&mut self) {
        // This child only; `kill_on_drop` reaps it.
        if let Err(e) = self.child.start_kill() {
            log::debug!("idb_companion was already gone: {e}");
        }
    }
}

/// Input to an attached simulator.
pub(super) struct Controls {
    client: Client,
    /// The screen's size in points, upright.
    screen: watch::Receiver<proto::ScreenDimensions>,
    udid: DeviceId,
    turned: Turned,
    _companion: Companion,
}

impl Controls {
    pub(super) async fn send(&mut self, input: Input) -> Result<()> {
        let events = match input {
            Input::Touch { phase, x, y } => {
                let (x, y) = self.turned.get().upright(x, y);
                let dimensions = beep_proto::watched::read(&self.screen, |screen| *screen);
                let width = u32::try_from(dimensions.width_points)
                    .context("screen width exceeds input limits")?;
                let height = u32::try_from(dimensions.height_points)
                    .context("screen height exceeds input limits")?;
                let (x, y) = point(x, y, f64::from(width), f64::from(height));
                let direction = match phase {
                    // idb drags as a run of touches down, then one up.
                    TouchPhase::Down | TouchPhase::Move => HidDirection::Down,
                    TouchPhase::Up => HidDirection::Up,
                };
                let touch = Action::Touch(HidTouch {
                    point: Some(Point { x, y }),
                });
                vec![press(touch, direction)]
            }
            Input::Text { text } => {
                if text.len() > MAX_TEXT {
                    bail!("text over {MAX_TEXT} bytes");
                }

                typed(&text)
            }
            Input::Key { key } => tap_key(key_code(key), false),
            Input::Button {
                button: Button::Rotate,
            } => return self.rotate().await,
            Input::Button { button } => match button_type(button) {
                Some(button) => {
                    let button = || {
                        Action::Button(HidButton {
                            button: button.into(),
                        })
                    };
                    vec![
                        press(button(), HidDirection::Down),
                        press(button(), HidDirection::Up),
                    ]
                }
                None => return Ok(()),
            },
            other => bail!("unknown input {other:?}"),
        };

        self.client.hid(futures::stream::iter(events)).await?;
        Ok(())
    }

    /// Turns the simulator a quarter turn, with a HID event: setting the
    /// orientation fails on simulators from Xcode 26.
    async fn rotate(&mut self) -> Result<()> {
        let next = self.turned.get().next();
        let orientation = match next {
            Turn::Upright => HidOrientationType::Portrait,
            Turn::Left => HidOrientationType::LandscapeRight,
            Turn::UpsideDown => HidOrientationType::PortraitUpsideDown,
            Turn::Right => HidOrientationType::LandscapeLeft,
        };
        let event = HidEvent {
            event: Some(Event::Orientation(HidOrientation {
                orientation: orientation.into(),
            })),
        };
        self.client.hid(futures::stream::iter([event])).await?;
        self.turned.set(next);
        lock(&TURNS).insert(self.udid.0.clone(), next);
        Ok(())
    }
}

impl Turn {
    /// A quarter turn on.
    fn next(self) -> Self {
        match self {
            Self::Upright => Self::Left,
            Self::Left => Self::UpsideDown,
            Self::UpsideDown => Self::Right,
            Self::Right => Self::Upright,
        }
    }
}

fn press(action: Action, direction: HidDirection) -> HidEvent {
    HidEvent {
        event: Some(Event::Press(HidPress {
            action: Some(HidPressAction {
                action: Some(action),
            }),
            direction: direction.into(),
        })),
    }
}

/// A key pressed and let go, with shift held around it when `shifted`.
fn tap_key(code: u64, shifted: bool) -> Vec<HidEvent> {
    let key = |keycode| Action::Key(HidKey { keycode });
    let tap = [
        press(key(code), HidDirection::Down),
        press(key(code), HidDirection::Up),
    ];

    if !shifted {
        return tap.into();
    }

    let mut events = vec![press(key(SHIFT), HidDirection::Down)];
    events.extend(tap);
    events.push(press(key(SHIFT), HidDirection::Up));
    events
}

/// The key presses that type `text` on a US keyboard. Characters it has no
/// key for are left out.
fn typed(text: &str) -> Vec<HidEvent> {
    text.chars()
        .filter_map(|c| {
            let key = us_key(c);

            if key.is_none() {
                log::debug!("no key types {c:?}");
            }

            key
        })
        .flat_map(|(code, shifted)| tap_key(code, shifted))
        .collect()
}

/// The USB HID usage that types `c` on a US keyboard, and whether shift is
/// held for it.
fn us_key(c: char) -> Option<(u64, bool)> {
    const SHIFTED_DIGITS: &str = "!@#$%^&*()";
    const PUNCTUATION: [(char, char, u64); 11] = [
        ('-', '_', 45),
        ('=', '+', 46),
        ('[', '{', 47),
        (']', '}', 48),
        ('\\', '|', 49),
        (';', ':', 51),
        ('\'', '"', 52),
        ('`', '~', 53),
        (',', '<', 54),
        ('.', '>', 55),
        ('/', '?', 56),
    ];
    let from = |base: char| u64::from(u32::from(c) - u32::from(base));

    match c {
        'a'..='z' => Some((4 + from('a'), false)),
        'A'..='Z' => Some((4 + from('A'), true)),
        '1'..='9' => Some((30 + from('1'), false)),
        '0' => Some((39, false)),
        '\n' => Some((40, false)),
        '\t' => Some((43, false)),
        ' ' => Some((44, false)),
        _ => {
            if let Some(at) = SHIFTED_DIGITS.chars().position(|digit| digit == c) {
                return Some((30 + at as u64, true));
            }

            PUNCTUATION.iter().find_map(|&(plain, shifted, code)| {
                (c == plain)
                    .then_some((code, false))
                    .or((c == shifted).then_some((code, true)))
            })
        }
    }
}

fn key_code(key: Key) -> u64 {
    match key {
        Key::Return => 40,
        Key::Escape => 41,
        Key::Delete => 42,
        Key::Tab => 43,
        Key::Right => 79,
        Key::Left => 80,
        Key::Down => 81,
        Key::Up => 82,
    }
}

/// The simulator's button for `button`, `None` for one an iPhone lacks.
fn button_type(button: Button) -> Option<HidButtonType> {
    match button {
        Button::Home => Some(HidButtonType::Home),
        Button::Power => Some(HidButtonType::Lock),
        Button::VolumeUp => Some(HidButtonType::VolumeUp),
        Button::VolumeDown => Some(HidButtonType::VolumeDown),
        Button::Back | Button::AppSwitch | Button::Rotate => None,
    }
}

#[cfg(test)]
#[path = "../../tests/run_device_ios.rs"]
mod tests;
