//! The `live` event: what the bar and the orb hear about live mode.

use super::Waiting;
use crate::beep::Decision;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum LiveEvent {
    View {
        phase: &'static str,
        /// What the user said so far this turn.
        words: String,
        /// Beep's sentence on screen, and the one before it.
        line: Option<String>,
        prev: Option<String>,
        /// The tool step running.
        step: Option<String>,
        /// Whether Beep's voice is playing, filler or answer.
        speaking: bool,
        pause_sends: bool,
        session: Option<String>,
        /// The voice note's title, when one started live mode.
        title: Option<String>,
        /// Push to talk: nothing listens between holds.
        hold_only: bool,
        /// Option came up and the last words are on their way.
        finishing: bool,
        /// The sheet of what to say is open.
        sheet: bool,
    },
    Level {
        level: f32,
    },
    Toast {
        text: String,
    },
    /// A pause sends after `ms`, or no longer will.
    Soon {
        ms: Option<u64>,
    },
    /// A spoken message reached `session` and its reply, `run`, started.
    /// The bar follows the run into the chat under the dock.
    Sent {
        session: String,
        text: String,
        run: String,
    },
    Left {
        draft: Option<String>,
        session: Option<String>,
        quiet: bool,
        failure: Option<String>,
    },
    /// The reply waits for the user to allow an action.
    Approval(Waiting),
    /// An approval no longer waits: answered as background work, answered
    /// elsewhere, or left unanswered by a new message.
    Withdrawn {
        query_id: String,
    },
    /// An approval was answered, in the bar or the orb, resuming `run`.
    Answered {
        session: String,
        query_id: String,
        run: String,
        decision: Decision,
    },
}

#[cfg(test)]
#[path = "../tests/voice_event.rs"]
mod tests;
