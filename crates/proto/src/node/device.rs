//! `device/1`: the Android emulators and iOS simulators of a node, shown and
//! used from a desktop. A stream opens with one `Request`. `List`, `Boot`
//! and `Shutdown` get one `Reply` each. `Attach` turns the stream into a live
//! view: the node sends a `Frame` whenever the screen changes, newest only,
//! and the desktop sends `Input` until it closes the stream or the node sends
//! `Ended`. `Platforms` gets one `Reply::Platforms`: the mobile platforms a
//! checkout builds for. `FollowPhones` turns the stream into the machine's
//! phone list, sent now and on every change. `Logs` gets one `Reply::Logs`:
//! a device's newest log lines.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL: &str = "device/1";

/// The longest side a frame may be asked for, in pixels.
pub const MAX_SIDE: u32 = 2048;
/// The most one `Input::Text` may carry, in bytes.
pub const MAX_TEXT: usize = 4096;
/// The most log lines one `Logs` answers with.
pub const MAX_LOG_LINES: u32 = 1000;

/// An emulator, simulator or phone: an Android AVD name, an iOS simulator
/// UDID or a phone's adb serial.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub String);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the desktop sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The devices the node can show, running or not.
    List {},
    /// Starts device `id`; the reply comes once it is starting.
    Boot {
        id: DeviceId,
    },
    Shutdown {
        id: DeviceId,
    },
    /// Follows device `id`'s screen, each frame scaled so its longer side is
    /// at most `max_side` pixels.
    Attach {
        id: DeviceId,
        max_side: u32,
    },
    /// Input to the attached device.
    Input {
        input: Input,
    },
    /// The platforms the app in folder `root` builds for, found by its
    /// files.
    Platforms {
        root: String,
    },
    /// The machine's phones now, then again each time one comes or goes,
    /// as `Reply::Devices`, until the desktop closes the stream.
    FollowPhones {},
    /// Device `id`'s newest log lines containing `filter` (any case; all
    /// lines when empty), at most `lines` of them, as `Reply::Logs`.
    Logs {
        id: DeviceId,
        lines: u32,
        filter: String,
    },
}

/// Something the user did on the attached device's screen or buttons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Input {
    /// A finger at `x`, `y`, each a fraction of the screen's width and
    /// height from its top left corner.
    Touch {
        phase: TouchPhase,
        x: f32,
        y: f32,
    },
    /// Text typed into whatever has focus.
    Text {
        text: String,
    },
    Key {
        key: Key,
    },
    Button {
        button: Button,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
}

/// A key with no text of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Return,
    Delete,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
}

/// A hardware button. A device without one ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Button {
    Home,
    /// Android only.
    Back,
    /// Android's recent apps.
    AppSwitch,
    Power,
    VolumeUp,
    VolumeDown,
    /// Turns the device a quarter turn.
    Rotate,
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    Devices {
        devices: Vec<Device>,
    },
    Done {},
    Frame(Frame),
    /// The attached device stopped, or its view could not go on.
    Ended {
        reason: String,
    },
    Failed {
        reason: String,
    },
    /// Android first; empty when the folder is no mobile app.
    Platforms {
        platforms: Vec<Platform>,
    },
    /// The answer to `Logs`, oldest line first.
    Logs {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    /// Such as "Pixel 9 Pro" or "iPhone 17".
    pub name: String,
    pub platform: Platform,
    pub state: DeviceState,
    /// Such as "Android 16" or "iOS 26.5", when known.
    pub os: Option<String>,
    /// A phone reached over adb, which stays on: it has no boot or shut
    /// down.
    #[serde(default)]
    pub phone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    Android,
    Ios,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceState {
    Off,
    Booting,
    On,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    #[serde(with = "serde_bytes")]
    pub jpeg: Vec<u8>,
    /// The image's size in pixels.
    pub width: u32,
    pub height: u32,
}

/// Never shows the image bytes.
impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("bytes", &self.jpeg.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}
