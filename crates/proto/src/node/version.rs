//! `version/1`: the node says which beep-node it runs, in one message, and
//! finishes the stream. A node from before it declines the protocol, which
//! reads as a version not known. `status/1` is not changed to carry it.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "version/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// Such as "0.2.3".
    pub version: String,
}
