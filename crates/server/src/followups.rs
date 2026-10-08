//! Follow-up replies shown under the assistant's reply. Jev decides
//! whether the reply invites one, then one plain completion writes them as a
//! Markdown list, one follow-up per line.
use crate::{
    decision::{Criteria, DecisionTool},
    memories::{Turn, clip},
    starters::{Generator, list_items},
    tools::replies_output,
};
use chrono::{DateTime, Duration, Utc};
use flashmind::core::{Conversation, EntryKind};
use std::{collections::HashMap, fmt};

/// How likely a follow-up has to be useful before one is written.
const THRESHOLD: f64 = 0.8;

/// An earlier turn older than this is left out, so a stale detail from
/// yesterday does not turn into a chip today.
const EARLIER_WINDOW_MINUTES: i64 = 30;
const MAX_CONTEXT_CHARS: usize = 1500;
const MAX_RESULT_CHARS: usize = 600;
const MAX_RESULTS: usize = 6;

const QUESTION: &str = "Would the user likely want tappable follow-ups under this reply: a question to dig deeper into the topic, or something the assistant could do next with what it just said?";

const CRITERIA: Criteria = Criteria {
    yes: "The reply explains a topic, answers a factual question, recommends or compares things, or finds something that leads to an action like a reminder, directions, a calendar event or a search.",
    no: "Small talk, thanks or goodbyes, a finished task with nothing left to do, or a reply that already asks the user a question.",
};

const SYSTEM: &str = "You write follow-ups, shown as tappable chips under an assistant's \
reply. Answer with a Markdown list of 2 or 3 items, one per line starting with \"- \", and \
nothing else. Write 3 only when each one is clearly worth tapping. Each item is a message the \
user would send next, in the user's language, at most 32 characters, no quotes.

You read what the assistant knew (time, place, connected apps, what it remembers), the latest \
exchange, and what its tools returned. A good follow-up is specific to this reply and is one of:
- something the assistant can do with it: set a reminder or an alarm, add a calendar event, get \
directions, search the web, or read or send email in a connected app. When the reply is about \
something later today or on a date, a reminder before it is often the best one.
- a question the reply raises but does not answer.

Never suggest a question that the reply, the tool results or the context already answer, an \
action the assistant cannot do, or a topic from an earlier turn. A follow-up that would fit any \
reply is a bad one.";

/// What Jev and the writer read about the turn.
pub struct Input<'a> {
    /// The time, place, connected apps and remembered facts the assistant had.
    pub context: Option<&'a str>,
    /// The turn before, only when it was recent.
    pub earlier: Option<&'a Turn>,
    pub latest: &'a Turn,
    pub results: Vec<ToolOutput<'a>>,
}

/// One tool result from the turn.
pub struct ToolOutput<'a> {
    pub tool: &'a str,
    pub output: &'a str,
}

impl<'a> Input<'a> {
    /// The input for the turn that started at `turn_from` in `conversation`.
    pub fn new(
        conversation: &'a Conversation,
        turn_from: DateTime<Utc>,
        earlier: &'a [Turn],
        latest: &'a Turn,
    ) -> Self {
        let entries = conversation.entries();
        // The context is written with the message, so it may share its timestamp.
        let context = entries
            .iter()
            .rev()
            .filter(|entry| entry.timestamp <= turn_from)
            .find_map(|entry| match &entry.kind {
                EntryKind::Developer {
                    content, tag: None, ..
                } => Some(content.as_str()),
                _ => None,
            });
        let recent = entries
            .iter()
            .rev()
            .filter(|entry| entry.timestamp < turn_from)
            .find(|entry| entry.is_user())
            .is_some_and(|asked| {
                turn_from - asked.timestamp <= Duration::minutes(EARLIER_WINDOW_MINUTES)
            });
        let names: HashMap<&str, &str> = entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                EntryKind::Assistant {
                    tool_calls: Some(calls),
                    ..
                } => Some(calls),
                _ => None,
            })
            .flatten()
            .map(|call| (call.id.as_str(), call.name.as_str()))
            .collect();
        let results = entries
            .iter()
            .filter(|entry| entry.timestamp >= turn_from)
            .filter_map(|entry| match &entry.kind {
                EntryKind::Tool { call_id, output } => Some(ToolOutput {
                    tool: names.get(call_id.as_str())?,
                    output,
                }),
                _ => None,
            })
            .take(MAX_RESULTS)
            .collect();
        Self {
            context,
            earlier: earlier.last().filter(|_| recent),
            latest,
            results,
        }
    }
}

impl fmt::Display for Input<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(context) = self.context {
            writeln!(
                f,
                "What the assistant knew:\n{}\n",
                clip(context, MAX_CONTEXT_CHARS)
            )?;
        }
        if let Some(turn) = self.earlier {
            writeln!(f, "Earlier:\n{turn}\n")?;
        }
        write!(f, "Now:\n{}", self.latest)?;
        if !self.results.is_empty() {
            write!(f, "\n\nWhat its tools returned:")?;
            for result in &self.results {
                write!(
                    f,
                    "\n- {}: {}",
                    result.tool,
                    clip(result.output, MAX_RESULT_CHARS)
                )?;
            }
        }
        Ok(())
    }
}

/// The `suggest_replies` result to add under the reply, or `None` when the
/// turn does not call for one or writing it failed.
pub async fn write(
    decision: &DecisionTool,
    generator: Generator<'_>,
    state: String,
) -> Option<String> {
    if score(decision, &state).await? < THRESHOLD {
        return None;
    }
    draft(generator, state).await
}

/// Jev's estimate that the turn calls for follow-ups.
pub async fn score(decision: &DecisionTool, state: &str) -> Option<f64> {
    match decision.probability_with(state, QUESTION, &CRITERIA).await {
        Ok(probability) => {
            log::info!("follow-up score {probability:.2}");
            Some(probability)
        }
        Err(error) => {
            log::warn!("follow-up score failed: {error}");
            None
        }
    }
}

/// The follow-ups for `state` as a `suggest_replies` result, whatever Jev
/// thinks of the turn.
pub async fn draft(generator: Generator<'_>, state: String) -> Option<String> {
    let answer = match generator.text(SYSTEM, state).await {
        Ok(answer) => answer,
        Err(error) => {
            log::warn!("unable to write follow-ups: {error}");
            return None;
        }
    };
    let replies = list_items(&answer)
        .into_iter()
        .map(|item| item.trim_matches(['"', '\'', '“', '”']).trim().to_owned())
        .collect();
    let output = replies_output(replies);
    if output.is_none() {
        log::warn!("fewer than two usable follow-ups");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::{core::ConversationEntry, types::ToolCall};
    use serde_json::json;

    fn turn(message: &str, reply: &str) -> Turn {
        Turn {
            message: message.into(),
            reply: reply.into(),
            actions: Vec::new(),
        }
    }

    /// `entry` stamped `minutes` after the start of the chat.
    fn at(start: DateTime<Utc>, minutes: i64, mut entry: ConversationEntry) -> ConversationEntry {
        entry.timestamp = start + Duration::minutes(minutes);
        entry
    }

    fn chat(gap: i64) -> (Conversation, DateTime<Utc>) {
        let start = Utc::now();
        let call = ToolCall {
            id: "cal".into(),
            name: "outlook_get_calendar_view".into(),
            arguments: json!({}),
        };
        let mut conversation = Conversation::new();
        for entry in [
            at(
                start,
                0,
                ConversationEntry::system_message("Time: yesterday"),
            ),
            at(start, 0, ConversationEntry::user("When is the meeting?")),
            at(start, 0, ConversationEntry::assistant("Tomorrow at 17:30.")),
            at(start, gap, ConversationEntry::system_message("Time: today")),
            at(start, gap, ConversationEntry::reminder("A reminder")),
            at(start, gap, ConversationEntry::user("Where is it?")),
            at(
                start,
                gap,
                ConversationEntry::assistant_with_tool_calls("", vec![call], None),
            ),
            at(start, gap, ConversationEntry::tool("cal", "Room 12")),
            at(start, gap, ConversationEntry::assistant("Room 12.")),
        ] {
            conversation.add(entry);
        }
        (conversation, start + Duration::minutes(gap))
    }

    #[test]
    fn reads_the_turns_context_and_tool_results() {
        let earlier = [turn("When is the meeting?", "Tomorrow at 17:30.")];
        let latest = turn("Where is it?", "Room 12.");
        let (conversation, turn_from) = chat(5);
        let input = Input::new(&conversation, turn_from, &earlier, &latest);
        assert_eq!(input.context, Some("Time: today"));
        assert!(input.earlier.is_some());
        assert_eq!(input.results.len(), 1);
        assert_eq!(input.results[0].tool, "outlook_get_calendar_view");
        assert_eq!(input.results[0].output, "Room 12");
        assert!(input.to_string().contains("What its tools returned:"));
    }

    #[test]
    fn leaves_out_an_old_earlier_turn() {
        let earlier = [turn("When is the meeting?", "Tomorrow at 17:30.")];
        let latest = turn("Where is it?", "Room 12.");
        let (conversation, turn_from) = chat(24 * 60);
        let input = Input::new(&conversation, turn_from, &earlier, &latest);
        assert!(input.earlier.is_none());
        assert!(!input.to_string().contains("Earlier:"));
    }
}
