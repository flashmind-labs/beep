//! The credentials a node uses for the user, and how a session stream opens.

use crate::SecretText;
use serde::{Deserialize, Serialize};

/// What a node uses on the user's behalf.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineCredentials {
    /// Env vars every process the node starts gets, such as `GH_TOKEN`.
    pub env: Vec<EnvSecret>,
    /// The Forgejo and Gitea servers teacli signs in to.
    pub forgejo: Vec<ForgejoLogin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvSecret {
    pub name: String,
    pub value: SecretText,
}

/// One Forgejo or Gitea server and the token for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgejoLogin {
    /// Its base URL, such as `https://git.example.com`, without a trailing
    /// slash.
    pub url: String,
    pub token: SecretText,
}

/// The first message on a session stream: the protocol it speaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub protocol: String,
}

/// The node's answer to `Open`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Opened {
    Ready {},
    Refused { reason: String },
}
