//! `sign-in/1`: a desktop has a node run a coding agent's own sign-in, so a
//! machine with no screen (or no one at it) signs in from the desktop. The
//! node prints nothing of its own: it passes on the link and device code
//! the agent printed, and writes a code the user pastes to the agent's
//! input. One `Request` per stream, one `State` back.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

pub const PROTOCOL: &str = "sign-in/1";

/// An agent whose sign-in a node runs. Named on the wire by its string, so
/// a node that does not know a newer one says so in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Tool {
    /// `claude auth login`: a link, then a code the page shows to paste.
    Claude,
    /// `codex login --device-auth`: a link and a one-time code to type there.
    Codex,
    /// `opencode auth login` with ChatGPT's headless method: a link and a
    /// one-time code, as Codex.
    Opencode,
}

/// What the desktop sends. `tool` is a `Tool` by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Starts the sign-in, or answers with the one under way, and waits for
    /// its link.
    Start { tool: String },
    /// What the user pasted, for a sign-in that asks for a code.
    Answer { tool: String, text: String },
    /// How the sign-in stands.
    State { tool: String },
    /// Stops the sign-in under way.
    Cancel { tool: String },
}

/// How one tool's sign-in stands on the node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum State {
    /// None was started since the node started, or the last one was
    /// cancelled.
    Idle {},
    /// Started; the agent has not printed its link yet.
    Starting {},
    /// The agent waits for the user to open `url`.
    Waiting {
        url: String,
        /// The code to type on the page, for a device sign-in.
        code: Option<String>,
        /// The agent reads a code the page shows back.
        asks_code: bool,
    },
    SignedIn {},
    /// It ended without signing in, or could not start; in words for the
    /// user.
    Failed {
        reason: String,
    },
}

impl State {
    /// Still under way.
    pub fn running(&self) -> bool {
        matches!(self, Self::Starting {} | Self::Waiting { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lenient;

    #[test]
    fn requests_read_by_position() {
        let named = rmp_serde::to_vec(&Request::Answer {
            tool: "claude".into(),
            text: "abc".into(),
        })
        .expect("encodes");
        #[derive(Serialize)]
        enum Tuple {
            Answer(&'static str, &'static str),
        }
        let positional = rmp_serde::to_vec(&Tuple::Answer("claude", "abc")).expect("encodes");
        assert_eq!(named, positional);
    }

    #[test]
    fn a_waiting_state_round_trips() {
        let state = State::Waiting {
            url: "https://auth.openai.com/codex/device".into(),
            code: Some("ABCD-12345".into()),
            asks_code: false,
        };
        let bytes = rmp_serde::to_vec(&state).expect("encodes");
        let read: State = lenient::from_slice(&bytes).expect("decodes");
        assert_eq!(read, state);
        assert!(read.running());
    }

    #[test]
    fn tools_go_by_name() {
        assert_eq!("opencode".parse::<Tool>().ok(), Some(Tool::Opencode));
        assert_eq!(Tool::Claude.to_string(), "claude");
        assert!("gemini".parse::<Tool>().is_err());
    }
}
