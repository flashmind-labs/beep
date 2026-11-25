use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};
use thiserror::Error;

const ID_LEN: std::ops::RangeInclusive<usize> = 2..=40;

/// A plugin's id, which is also its folder name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PluginId(String);

#[derive(Debug, Error)]
#[error(
    "invalid plugin id {0:?}: use 2 to 40 lowercase letters, digits or dashes, starting with a letter"
)]
pub struct InvalidPluginId(String);

impl FromStr for PluginId {
    type Err = InvalidPluginId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut chars = text.chars();
        let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_lowercase());
        let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');

        if ID_LEN.contains(&text.len()) && starts_with_letter && rest_ok {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidPluginId(text.to_owned()))
        }
    }
}

impl TryFrom<String> for PluginId {
    type Error = InvalidPluginId;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<PluginId> for String {
    fn from(id: PluginId) -> Self {
        id.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a plugin may do. Unknown permissions fail the manifest.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Display,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum Permission {
    /// Place UI in a slot.
    #[serde(rename = "ui.add")]
    #[strum(serialize = "ui.add")]
    UiAdd,
    /// Follow the app's session topics, `agents` and `agent`.
    #[serde(rename = "sessions.read")]
    #[strum(serialize = "sessions.read")]
    SessionsRead,
    /// Give coding agents tools, declared in `tools` and set up with `beep.tool`.
    #[serde(rename = "tools")]
    #[strum(serialize = "tools")]
    Tools,
    /// Restyle the app with the tokens declared in `style`.
    #[serde(rename = "style")]
    #[strum(serialize = "style")]
    Style,
}

/// A named place in the app where plugins put UI. A slot can appear many times
/// on the page, each time with its own context.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    EnumString,
    AsRefStr,
    Display,
)]
pub enum Slot {
    /// The edge of the session's message box. One instance.
    #[serde(rename = "composer")]
    #[strum(serialize = "composer")]
    Composer,
    /// A session's row in the session list. Its nodes cannot be interactive,
    /// since the whole row is a button.
    #[serde(rename = "session.row")]
    #[strum(serialize = "session.row")]
    SessionRow,
    /// The open session's header.
    #[serde(rename = "session.header")]
    #[strum(serialize = "session.header")]
    SessionHeader,
}

impl Slot {
    /// Whether nodes stand on the slot's edge, placed by `align`, `offset` and
    /// `sink`, rather than in normal flow.
    pub fn is_edge(self) -> bool {
        matches!(self, Slot::Composer)
    }

    /// Whether buttons, toggles and inputs may appear in the slot.
    pub fn is_interactive(self) -> bool {
        !matches!(self, Slot::SessionRow)
    }
}
