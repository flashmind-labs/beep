use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use thiserror::Error;

use crate::topic::{Topic, TopicError};
use crate::ui::Slot;

/// File name of the manifest inside a plugin folder.
pub const MANIFEST_FILE: &str = "beep-plugin.json";

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
pub enum Permission {
    /// Place UI in a slot.
    #[serde(rename = "ui.add")]
    #[strum(serialize = "ui.add")]
    UiAdd,
    /// Follow the app's session topics, `agents` and `agent`.
    #[serde(rename = "sessions.read")]
    #[strum(serialize = "sessions.read")]
    SessionsRead,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InvalidActivation {
    #[error("invalid activation {0:?}: use \"startup\", \"slot:<slot>\" or \"topic:ui.<name>\"")]
    Unknown(String),
    #[error("invalid activation topic: {0}")]
    Topic(#[from] TopicError),
}

/// When a plugin starts. A plugin starts when any of its activations holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Activation {
    /// As soon as the host starts, and always running.
    Startup,
    /// When the page reports the first instance of the slot. The plugin stops
    /// after every slot it lists has been gone for a while.
    Slot(Slot),
    /// When the page first publishes the local topic (`ui.*`).
    Topic(Topic),
}

const SLOT_PREFIX: &str = "slot:";
const TOPIC_PREFIX: &str = "topic:";

impl FromStr for Activation {
    type Err = InvalidActivation;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text == "startup" {
            return Ok(Self::Startup);
        }
        if let Some(slot) = text.strip_prefix(SLOT_PREFIX) {
            return slot
                .parse()
                .map(Self::Slot)
                .map_err(|_| InvalidActivation::Unknown(text.to_owned()));
        }
        if let Some(name) = text.strip_prefix(TOPIC_PREFIX) {
            return Ok(Self::Topic(Topic::local(name)?));
        }

        Err(InvalidActivation::Unknown(text.to_owned()))
    }
}

impl fmt::Display for Activation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Startup => f.write_str("startup"),
            Self::Slot(slot) => write!(f, "{SLOT_PREFIX}{slot}"),
            Self::Topic(topic) => write!(f, "{TOPIC_PREFIX}{}", topic.name),
        }
    }
}

impl<'de> Deserialize<'de> for Activation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;

        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The parsed `beep-plugin.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: PluginId,
    pub name: String,
    pub version: semver::Version,
    #[serde(default)]
    pub description: String,
    /// Source file relative to the plugin folder.
    #[serde(default = "default_entry")]
    pub entry: PathBuf,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// When the plugin starts. Default: `["startup"]`.
    #[serde(default = "default_activation")]
    pub activation: Vec<Activation>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid {MANIFEST_FILE}: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("plugin id {id} does not match its folder {folder:?}")]
    IdMismatch { id: PluginId, folder: String },
    #[error("entry {} must be a relative path inside the plugin folder", .0.display())]
    Entry(PathBuf),
    #[error("activation needs at least one entry")]
    NoActivation,
}

impl Manifest {
    /// Reads and validates the manifest in a plugin folder.
    pub fn read(dir: &Path) -> Result<Self, ManifestError> {
        let path = dir.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| ManifestError::Read {
            path: path.clone(),
            source,
        })?;

        let manifest = Self::parse(&text)?;
        let folder = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if manifest.id.to_string() != folder {
            return Err(ManifestError::IdMismatch {
                id: manifest.id,
                folder,
            });
        }

        Ok(manifest)
    }

    /// Parses and validates manifest text.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_str(text)?;

        let stays_inside = manifest
            .entry
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
        if !stays_inside || manifest.entry.as_os_str().is_empty() {
            return Err(ManifestError::Entry(manifest.entry));
        }

        if manifest.activation.is_empty() {
            return Err(ManifestError::NoActivation);
        }

        Ok(manifest)
    }

    /// Whether the plugin runs from the start and never idles out.
    pub fn starts_at_launch(&self) -> bool {
        self.activation.contains(&Activation::Startup)
    }

    pub fn allows(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }
}

fn default_activation() -> Vec<Activation> {
    vec![Activation::Startup]
}

fn default_entry() -> PathBuf {
    PathBuf::from("main.ts")
}

#[cfg(test)]
#[path = "tests/manifest.rs"]
mod tests;
