//! A stretch of a chat as the watchers read it.

use crate::{
    memories::clip,
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
mod tests {
    use super::*;
    use crate::store::TurnRecord;
    use uuid::Uuid;

    fn message(id: i64, seconds: i64, body: StoredBody) -> StoredMessage {
        StoredMessage {
            id,
            chat_id: Uuid::nil(),
            turn_id: None,
            sent_at: DateTime::UNIX_EPOCH + TimeDelta::seconds(seconds),
            author_id: None,
            body,
            images: Vec::new(),
        }
    }

    fn user(id: i64, seconds: i64, content: &str) -> StoredMessage {
        message(
            id,
            seconds,
            StoredBody::User {
                content: content.into(),
                turn: TurnRecord::default(),
            },
        )
    }

    fn beep(id: i64, seconds: i64, content: &str) -> StoredMessage {
        message(
            id,
            seconds,
            StoredBody::Assistant {
                content: content.into(),
            },
        )
    }

    #[test]
    fn numbers_messages_and_marks_failed_calls_and_slow_turns() {
        let messages = [
            user(5, 0, "Book padel"),
            message(
                6,
                50,
                StoredBody::Tool {
                    call_id: "c1".into(),
                    name: "web_search".into(),
                    output: "timeout".into(),
                    success: false,
                    description: String::new(),
                },
            ),
            beep(7, 60, "I could not find courts."),
            user(8, 70, "no, I meant tomorrow"),
            beep(9, 75, "Booked."),
        ];
        let arguments = HashMap::from([("c1".to_owned(), "{\"q\":\"padel\"}".to_owned())]);
        let read = transcript(&messages, &arguments, BUDGET);
        assert_eq!(
            read.text,
            "[#5] User: Book padel\n[#6] [call web_search {\"q\":\"padel\"}] FAILED [result timeout]\n[#7] Beep: I could not find courts.\n[turn took 60s]\n\n[#8] User: no, I meant tomorrow\n[#9] Beep: Booked."
        );
        assert_eq!((read.first_seq, read.last_seq), (5, 9));
        assert!(read.talk);
    }

    #[test]
    fn stops_at_the_budget_but_always_reads_one() {
        let long = "x".repeat(100);
        let messages = [user(1, 0, &long), beep(2, 1, &long), user(3, 2, &long)];
        let read = transcript(&messages, &HashMap::new(), 250);
        assert_eq!((read.first_seq, read.last_seq), (1, 2));
        let one = transcript(&messages, &HashMap::new(), 10);
        assert_eq!((one.first_seq, one.last_seq), (1, 1));
        let none = transcript(&[], &HashMap::new(), 10);
        assert_eq!((none.first_seq, none.last_seq), (0, 0));
        assert!(!none.talk);
    }
}
