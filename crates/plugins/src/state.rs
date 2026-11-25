use std::fmt;

use serde::Serialize;

use crate::ident::Permission;

/// Where a plugin stands, as the page and agent tools read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum PluginState {
    /// Not started: nothing it is activated by has happened.
    Waiting,
    Running,
    /// Running, but no window is visible, so its timers and updates wait.
    Paused,
    /// Failed, went over its budget or has an invalid manifest. It shows
    /// nothing until its files change.
    Stopped {
        reason: String,
    },
    /// Turned off by the user.
    Disabled,
    /// The manifest asks for permissions the user has not allowed. It does
    /// not run until they are.
    NeedsApproval {
        /// The permissions that are not allowed yet.
        missing: Vec<Permission>,
    },
}

impl fmt::Display for PluginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Waiting => f.write_str("waiting to start"),
            Self::Running => f.write_str("running"),
            Self::Paused => f.write_str("paused while no Beep window is visible"),
            Self::Stopped { reason } => write!(f, "stopped ({reason})"),
            Self::Disabled => f.write_str("turned off"),
            Self::NeedsApproval { .. } => {
                f.write_str("waiting for the user's approval in Settings, Plugins")
            }
        }
    }
}
