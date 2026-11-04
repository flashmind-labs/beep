//! `ssh/1`: the node's own `ssh`, to add another machine. The server sends
//! one `Request` per stream and the node answers with one `Event`.

use crate::SecretText;
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "ssh/1";

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The hosts this node's ssh knows: `Host` names in `~/.ssh/config`
    /// without wildcards, then unhashed names in `~/.ssh/known_hosts`.
    Hosts {},
    /// Connects to `target` (`user@host` or a config alias) without
    /// prompting, installs beep-node there and links it with `token`
    /// (`beep-node setup`).
    Add { target: String, token: SecretText },
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    Hosts {
        hosts: Vec<String>,
    },
    /// beep-node runs on the target, linked with the token.
    Added {
        /// What the target said it runs, such as "Linux x86_64".
        system: String,
    },
    Failed {
        /// Which step failed and why, in words for the user.
        reason: String,
        /// As for `Added`; empty when it was never read.
        #[serde(default)]
        system: String,
        /// The failing command's last lines, the token taken out.
        #[serde(default)]
        output: Vec<String>,
    },
}
