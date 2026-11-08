//! A stretch of a chat as the watchers read it.

use crate::{
    memory::memories::clip,
    store::{StoredBody, StoredMessage},
};
use chrono::{DateTime, TimeDelta, Utc};
use std::{collections::HashMap, fmt::Write};

/// The most of a chat one review reads. What is left waits for the next.
pub(crate) const BUDGET: usize = 40_000;
/// How much of one message a review reads.
const MESSAGE_CLIP: usize = 3_000;
/// How much of a tool call's arguments and output a review reads.
const TOOL_CLIP: usize = 400;
/// A turn that took longer than this says how long.
const SLOW_TURN: TimeDelta = TimeDelta::seconds(30);

/// What a review reads.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Transcript {
    pub text: String,
    /// The ids of the first and last message read; both 0 when none was.
    pub first_seq: i64,
    pub last_seq: i64,
    /// Whether the user said anything in it.
    pub talk: bool,
}

/// The open turn, to say how long it took once it ends.
struct OpenTurn {
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
}

fn close_turn(text: &mut String, turn: Option<OpenTurn>) {
    if let Some(turn) = turn {
        let took = turn.ended - turn.started;
        if took > SLOW_TURN {
            let _ = write!(text, "\n[turn took {}s]", took.num_seconds());
        }
    }
}

/// Writes out `messages` until `budget` bytes are used, always taking the first.
pub(crate) fn transcript(
    messages: &[StoredMessage],
    arguments: &HashMap<String, String>,
    budget: usize,
) -> Transcript {
    let mut text = String::new();
    let mut first_seq = 0;
    let mut last_seq = 0;
    let mut talk = false;
    let mut turn: Option<OpenTurn> = None;
    for message in messages {
        let mut line = String::new();
        let id = message.id;
        match &message.body {
            StoredBody::User { content, .. } => {
                close_turn(&mut line, turn.take());
                let _ = write!(line, "\n\n[#{id}] User: {}", clip(content, MESSAGE_CLIP));
            }
            StoredBody::Scheduled { label, .. } => {
                close_turn(&mut line, turn.take());
                let _ = write!(line, "\n\n[#{id}] Task started: {}", clip(label, 200));
            }
            StoredBody::Assistant { content } => {
                let _ = write!(line, "\n[#{id}] Beep: {}", clip(content, MESSAGE_CLIP));
            }
            StoredBody::Tool {
                call_id,
                name,
                output,
                success,
                ..
            } => {
                let arguments = arguments
                    .get(call_id)
                    .map(|arguments| clip(arguments, TOOL_CLIP))
                    .unwrap_or_default();
                let failed = if *success { "" } else { " FAILED" };
                let _ = write!(
                    line,
                    "\n[#{id}] [call {name} {arguments}]{failed} [result {}]",
                    clip(output, TOOL_CLIP)
                );
            }
            StoredBody::Approval { approval, .. } => {
                let _ = write!(
                    line,
                    "\n[#{id}] [asked the user to allow: {} {}]",
                    approval.app, approval.action
                );
            }
            // Group messages are not watched.
            StoredBody::Member { .. } | StoredBody::Beep { .. } => continue,
        }
        if first_seq != 0 && text.len() + line.len() > budget {
            break;
        }
        match &message.body {
            StoredBody::User { .. } | StoredBody::Scheduled { .. } => {
                turn = Some(OpenTurn {
                    started: message.sent_at,
                    ended: message.sent_at,
                });
            }
            _ => {
                if let Some(turn) = &mut turn {
                    turn.ended = message.sent_at;
                }
            }
        }
        text.push_str(&line);
        talk |= matches!(message.body, StoredBody::User { .. });
        if first_seq == 0 {
            first_seq = id;
        }
        last_seq = id;
    }
    // Nothing more was read, so the open turn ends with what was.
    close_turn(&mut text, turn);
    Transcript {
        text: text.trim().to_owned(),
        first_seq,
        last_seq,
        talk,
    }
}

#[cfg(test)]
#[path = "../../tests/memory_watchers_transcript.rs"]
mod tests;
