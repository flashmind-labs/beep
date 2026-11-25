use std::fmt;
use std::num::NonZeroU32;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use thiserror::Error;

use crate::ident::{Permission, PluginId, Slot};
use crate::style::Style;
use crate::tool::{ToolSpec, ToolsError, check_all};
use crate::topic::{Topic, TopicError};

/// File name of the manifest inside a plugin folder.
pub const MANIFEST_FILE: &str = "beep-plugin.json";

/// The plugin API this Beep implements. A manifest asks for one with `api`.
/// API 2 adds `style`.
pub const API_VERSION: NonZeroU32 = NonZeroU32::new(2).expect("2 is not zero");

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
    /// The plugin API the plugin is written for. Default 1.
    #[serde(default = "default_api")]
    pub api: NonZeroU32,
    #[serde(default)]
    pub description: String,
    /// Source file relative to the plugin folder.
    #[serde(default = "default_entry")]
    pub entry: PathBuf,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// When the plugin starts. Default: `["startup"]`. A plugin with a `style`
    /// may list none, and then never runs code.
    #[serde(default = "default_activation")]
    pub activation: Vec<Activation>,
    /// The tools the plugin gives coding agents. Needs the `tools` permission.
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    /// The style tokens the plugin sets while it is enabled. Needs the
    /// `style` permission.
    #[serde(default)]
    pub style: Style,
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
    #[error("entry {} must be a .js file; build TypeScript to JavaScript before installing the plugin", .0.display())]
    JavaScriptEntry(PathBuf),
    #[error("needs a newer Beep: the plugin uses API {0}, this Beep knows up to {API_VERSION}")]
    Api(NonZeroU32),
    #[error("activation needs at least one entry")]
    NoActivation,
    #[error("style needs the style permission")]
    StylePermission,
    #[error("invalid tools: {0}")]
    Tools(#[from] ToolsError),
}

impl Manifest {
    /// Reads and validates the manifest in a plugin folder, whose name is the plugin's id.
    pub fn read(dir: &Path) -> Result<Self, ManifestError> {
        let manifest = Self::load(dir)?;
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

    /// Reads and validates the manifest in a folder of any name, such as a
    /// plugin that is not installed yet.
    pub fn load(dir: &Path) -> Result<Self, ManifestError> {
        let path = dir.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| ManifestError::Read {
            path: path.clone(),
            source,
        })?;

        Self::parse(&text)
    }

    /// Parses and validates manifest text.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_str(text)?;

        if manifest.api > API_VERSION {
            return Err(ManifestError::Api(manifest.api));
        }

        let stays_inside = manifest
            .entry
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
        if !stays_inside || manifest.entry.as_os_str().is_empty() {
            return Err(ManifestError::Entry(manifest.entry));
        }

        if !manifest.activation.is_empty()
            && manifest.entry.extension().and_then(|ext| ext.to_str()) != Some("js")
        {
            return Err(ManifestError::JavaScriptEntry(manifest.entry));
        }

        if manifest.activation.is_empty() && manifest.style.is_empty() {
            return Err(ManifestError::NoActivation);
        }

        if !manifest.style.is_empty() && !manifest.allows(Permission::Style) {
            return Err(ManifestError::StylePermission);
        }

        check_all(&manifest.tools, &manifest.permissions)?;

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

fn default_api() -> NonZeroU32 {
    API_VERSION
}

fn default_activation() -> Vec<Activation> {
    vec![Activation::Startup]
}

fn default_entry() -> PathBuf {
    PathBuf::from("main.js")
}

#[cfg(test)]
#[path = "tests/manifest.rs"]
mod tests;
