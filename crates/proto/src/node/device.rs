//! `device/1`: the Android emulators and iOS simulators of a node, shown and
//! used from a desktop. A stream opens with one `Request`. `List`, `Boot`
//! and `Shutdown` get one `Reply` each. `Attach` turns the stream into a live
//! view: the node sends a `Frame` whenever the screen changes, newest only,
//! and the desktop sends `Input` until it closes the stream or the node sends
//! `Ended`.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL: &str = "device/1";

/// The longest side a frame may be asked for, in pixels.
pub const MAX_SIDE: u32 = 2048;
/// The most one `Input::Text` may carry, in bytes.
pub const MAX_TEXT: usize = 4096;

/// An emulator or simulator: an Android AVD name or an iOS simulator UDID.
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
    Boot { id: DeviceId },
    Shutdown { id: DeviceId },
    /// Follows device `id`'s screen, each frame scaled so its longer side is
    /// at most `max_side` pixels.
    Attach { id: DeviceId, max_side: u32 },
    /// Input to the attached device.
    Input { input: Input },
}

/// Something the user did on the attached device's screen or buttons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Input {
    /// A finger at `x`, `y`, each a fraction of the screen's width and
    /// height from its top left corner.
    Touch { phase: TouchPhase, x: f32, y: f32 },
    /// Text typed into whatever has focus.
    Text { text: String },
    Key { key: Key },
    Button { button: Button },
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
    Devices { devices: Vec<Device> },
    Done {},
    Frame(Frame),
    /// The attached device stopped, or its view could not go on.
    Ended { reason: String },
    Failed { reason: String },
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
