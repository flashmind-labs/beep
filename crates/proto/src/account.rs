//! The account: models and agents, devices and invites.

use crate::ReasoningLevel;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// The models the caller can pick from and the one their turns use.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Models {
    /// Empty on servers from before every user could pick.
    pub choices: Vec<ModelChoice>,
    /// The `id` of the model the caller's turns use.
    pub selected: String,
    /// How long the model reasons on the caller's turns. Defaults so a
    /// phone's cached `Models` from before this field still decodes.
    #[serde(default)]
    pub thinking: ReasoningLevel,
    /// Jev picks the model and thinking of each of the caller's prompts.
    #[serde(default)]
    pub auto: bool,
    /// The server has models for Auto to pick from.
    #[serde(default)]
    pub auto_available: bool,
}

/// Whether agents are on for everyone, and the agents Jev picks from.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agents {
    pub enabled: bool,
    /// General first. Empty when the server has no agents set up, and then
    /// they cannot be turned on.
    pub agents: Vec<AgentInfo>,
}

/// An agent: a system prompt, a model and a thinking level for one kind of task.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Such as `getting_around`.
    pub id: String,
    pub name: String,
    /// What it handles.
    pub when: String,
    /// A `ModelChoice::id`.
    pub model: String,
    /// The least it thinks.
    pub thinking: ReasoningLevel,
}

/// The model and thinking Jev picked for a prompt.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoPick {
    /// A `ModelChoice::id`.
    pub model: String,
    pub thinking: ReasoningLevel,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChoice {
    /// The model as the server names it, for example `openrouter:openai/gpt-6-luna`.
    pub id: String,
    pub name: String,
    /// USD per million input tokens, when the provider lists it.
    pub input_price: Option<f64>,
    /// USD per million output tokens, when the provider lists it.
    pub output_price: Option<f64>,
    pub sees_images: bool,
    /// The server's default model.
    pub default: bool,
}

wire_enum! {
    /// What kind of client a key belongs to.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DeviceKind {
        0 Phone {}
        1 Desktop {}
    }
}

impl Default for DeviceKind {
    fn default() -> Self {
        Self::Phone {}
    }
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone {} => "phone",
            Self::Desktop {} => "desktop",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "phone" => Some(Self::Phone {}),
            "desktop" => Some(Self::Desktop {}),
            _ => None,
        }
    }
}

wire_enum! {
    /// Which app a request comes from.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum Platform {
        0 Android {}
        1 Ios {}
        2 Macos {}
    }
}

wire_enum! {
    /// The APNs server an iPhone's push token belongs to. Builds run from
    /// Xcode get sandbox tokens; TestFlight and App Store builds production ones.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ApnsEnvironment {
        0 Sandbox {}
        1 Production {}
    }
}

impl Platform {
    /// How the model is told where the user is writing from.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Android {} => "the Android app on their phone",
            Self::Ios {} => "the iOS app on their iPhone",
            Self::Macos {} => "the macOS app on their computer",
        }
    }
}

/// A key that signs into the account.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkedDevice {
    /// The device's endpoint id.
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    /// The account's own key, which cannot be unlinked.
    pub primary: bool,
    /// The key of the caller.
    pub this_device: bool,
    pub linked_at: SystemTime,
}

/// A code that lets another device join the account.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInvite {
    /// Ten letters and digits, as `link::format_join_code` shows them.
    pub code: String,
    /// The code as text for a QR code, as `link::join_uri` makes it.
    pub uri: String,
    pub expires_at: SystemTime,
}

/// The device behind an invite, shown before joining.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JoinPreview {
    pub name: String,
    pub kind: DeviceKind,
}
