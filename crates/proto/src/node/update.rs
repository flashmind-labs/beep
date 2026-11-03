//! `update/1`: the node installs the newest beep-node release over its own
//! binary and restarts its service, as `beep-node update` does on the
//! machine. Only the beep-node CLI offers it; the Mac app's node updates
//! with the app.
//!
//! The server sends one `Request::Update`; the node answers with one
//! `Event`, finishes the stream, and only then restarts, so the server
//! reads the answer before the connection drops.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "update/1";

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Installs the newest release if this node is behind it.
    Update {},
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    /// `version` is already the newest release, or newer.
    Newest { version: String },
    /// `to` replaced `from`.
    Installed {
        from: String,
        to: String,
        /// The service restarts on `to` once the stream is finished: the
        /// node drops off and connects again.
        restarting: bool,
        /// Why it does not restart, in words for the user; empty when it
        /// does.
        #[serde(default)]
        note: String,
    },
    /// Nothing was installed, or the install stopped; in words for the
    /// user.
    Failed { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip() {
        for event in [
            Event::Newest {
                version: "0.1.24".into(),
            },
            Event::Installed {
                from: "0.1.20".into(),
                to: "0.1.24".into(),
                restarting: true,
                note: String::new(),
            },
            Event::Failed {
                reason: "no build".into(),
            },
        ] {
            let body = rmp_serde::to_vec_named(&event).expect("encodes");
            assert_eq!(
                rmp_serde::from_slice::<Event>(&body).expect("decodes"),
                event
            );
        }
    }
}
