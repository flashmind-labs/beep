//! `desktop/1`: the screen, keyboard and mouse of a node that has a desktop,
//! such as the Mac app. Each stream is one session and each request gets one
//! `Reply`, in order. The session remembers its last screenshot, so a point
//! is in that screenshot's pixels (screen points before the first one), and
//! its last tree, so a click can name an element by its index.

use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

pub const PROTOCOL: &str = "desktop/1";

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The main display as a JPEG, scaled so its longer side is at most
    /// `max_side` pixels.
    Screenshot {
        max_side: u32,
    },
    /// The frontmost app's accessibility tree, at most `max_elements`
    /// elements, each numbered for `Target::Element`.
    Tree {
        max_elements: u32,
    },
    Click {
        target: Target,
        button: MouseButton,
        /// 2 for a double click.
        count: u8,
    },
    /// Types `text` into whatever has focus.
    Type {
        text: String,
    },
    Key {
        press: KeyPress,
    },
    /// Scrolls by lines: positive `dy` is down, positive `dx` is right.
    /// At `at` when given, else where the pointer is.
    Scroll {
        at: Option<Target>,
        dx: i32,
        dy: i32,
    },
    /// Opens an app by name or bundle id, or a URL in its app.
    Open {
        target: String,
    },
    /// The windows on screen, frontmost first.
    Windows {},
    /// Which of the permissions desktop control needs are granted.
    Permissions {},
}

/// Where a click or scroll lands.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// An element from the session's last tree.
    Element { index: u32 },
    /// A point in the last screenshot's pixels.
    Point { x: f64, y: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MouseButton {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Modifier {
    Command,
    Shift,
    Option,
    Control,
    Function,
}

impl Modifier {
    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "cmd" | "command" | "meta" | "super" => Self::Command,
            "shift" => Self::Shift,
            "opt" | "option" | "alt" => Self::Option,
            "ctrl" | "control" => Self::Control,
            "fn" => Self::Function,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "cmd",
            Self::Shift => "shift",
            Self::Option => "option",
            Self::Control => "ctrl",
            Self::Function => "fn",
        }
    }
}

/// One key with the modifiers held for it, written like `cmd+shift+t`.
/// `key` is a lowercase name: a letter, digit or symbol, or one of the
/// named keys such as `return`, `tab`, `escape`, `space`, `left` or `f5`.
/// The node refuses a name it does not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPress {
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<Modifier>,
}

impl FromStr for KeyPress {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim().to_lowercase();
        // A trailing "+" is the plus key itself, as in "cmd++".
        let (mods, key) = match text.strip_suffix("++") {
            Some(mods) => (mods, "+".to_owned()),
            None => match text.rsplit_once('+') {
                Some((mods, key)) => (mods, key.to_owned()),
                None => ("", text.clone()),
            },
        };
        let key = match key.as_str() {
            "plus" => "+".to_owned(),
            "enter" => "return".to_owned(),
            "esc" => "escape".to_owned(),
            "backspace" => "delete".to_owned(),
            _ => key,
        };
        if key.is_empty() {
            return Err(format!("{text:?} names no key"));
        }
        let mut modifiers = Vec::new();
        for word in mods.split('+').filter(|word| !word.is_empty()) {
            let modifier =
                Modifier::parse(word).ok_or_else(|| format!("{word:?} is not a modifier"))?;
            if !modifiers.contains(&modifier) {
                modifiers.push(modifier);
            }
        }
        modifiers.sort();
        Ok(Self { key, modifiers })
    }
}

impl fmt::Display for KeyPress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            write!(f, "{}+", modifier.as_str())?;
        }
        f.write_str(&self.key)
    }
}

/// The node's answer to one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    Screenshot(Screenshot),
    Tree(Tree),
    /// An action ran; `detail` says what it did, such as "pressed Save".
    Done {
        detail: String,
    },
    Windows {
        windows: Vec<Window>,
    },
    Permissions(Permissions),
    Failed {
        reason: String,
        /// Set when a permission the user has not granted stopped it.
        #[serde(default)]
        missing: Option<Permission>,
    },
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Screenshot {
    #[serde(with = "serde_bytes")]
    pub jpeg: Vec<u8>,
    /// The image's size in pixels.
    pub width: u32,
    pub height: u32,
    /// The display's size in points.
    pub screen_width: u32,
    pub screen_height: u32,
}

impl fmt::Debug for Screenshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Screenshot")
            .field("jpeg", &format_args!("{} bytes", self.jpeg.len()))
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    /// The frontmost app's name.
    pub app: String,
    /// Its focused window's title, when it has one.
    #[serde(default)]
    pub window: Option<String>,
    /// One element a line, indented by depth, such as
    /// `[12] button "Save"`. Only numbered elements can be clicked.
    pub text: String,
    /// How many elements got an index.
    pub elements: u32,
    /// Elements past `max_elements` were left out.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub app: String,
    /// Empty when the window has no title or Screen Recording is off.
    #[serde(default)]
    pub title: String,
    pub pid: i32,
    /// The frame in screen points.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Permissions {
    pub screen_recording: bool,
    pub accessibility: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    ScreenRecording,
    Accessibility,
}

impl Permission {
    /// Its name in System Settings, Privacy and Security.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ScreenRecording => "Screen Recording",
            Self::Accessibility => "Accessibility",
        }
    }
}
