//! Follow-up replies under the assistant's reply, gated by Jev and written by one completion.
use crate::{
    decision::{Criteria, DecisionTool},
    memories::{Turn, clip},
    starters::{Generator, list_items},
    tools::{choice_output, replies_output},
};
use chrono::{DateTime, Duration, Utc};
use flashmind::core::{Conversation, EntryKind};
use std::{
    collections::HashMap,
    fmt::{self, Write},
};

/// How likely a follow-up has to be useful before one is written.
const THRESHOLD: f64 = 0.8;

/// Earlier turns older than this are left out.
const EARLIER_WINDOW_MINUTES: i64 = 30;
const MAX_CONTEXT_CHARS: usize = 1500;
const MAX_RESULT_CHARS: usize = 600;
const MAX_RESULTS: usize = 6;

pub(crate) const QUESTION: &str = "Would the user likely want tappable follow-ups under the latest reply: a question to dig deeper into the topic, or something the assistant could do next with what it just said?";

pub(crate) const CRITERIA: Criteria = Criteria {
    yes: "The reply explains a topic, answers a factual question, recommends or compares things, or finds something that leads to an action like a reminder, directions, a calendar event or a search.",
    no: "Small talk, thanks or goodbyes, a finished task with nothing left to do, or a reply that already asks the user a question.",
};

const SYSTEM: &str = "You write follow-ups, shown as tappable chips under an assistant's \
reply. Answer with a Markdown list of 2 or 3 items, one per line starting with \"- \", and \
nothing else. Write 3 only when each one is clearly worth tapping. Each item is a message the \
user would send next, in the language the assistant's reply is written in (not the language \
of the place, the sources or the tool results), at most 40 characters, no quotes.

You read what the assistant knew (time, place, connected apps, what it remembers), the latest \
exchange, and what its tools returned. A good follow-up is specific to this reply and is one of:
- something the assistant can do with it: set a reminder or an alarm, add a calendar event, get \
directions, search the web, or read or send email in a connected app. When the reply is about \
something later today or on a date, a reminder before it is often the best one.
- a question the reply raises but does not answer.

Never suggest a question that the reply, the tool results or the context already answer, an \
action the assistant cannot do, or a topic from an earlier turn. A follow-up that would fit any \
reply is a bad one.";

const ANSWERS_SYSTEM: &str = "You write the answers to an assistant's question, shown as \
buttons the user taps instead of typing. Answer with a Markdown list of 2 to 4 items, one per \
line starting with \"- \", and nothing else. Each item is the user's whole reply, in the \
language the assistant's reply is written in (not the language of the place, the sources \
or the tool results), at most 40 characters, no quotes.

You read what the assistant knew (time, place, connected apps, what it remembers), the latest \
exchange, and what its tools returned. Answer the question the reply ends on:
- when it names options, one item per option, named so the assistant can tell them apart.
- when it asks for a time or a date, the likeliest concrete ones given the context, like \
\"Tomorrow 9am\" or \"Tonight at 8\".
- when it asks yes or no, the yes and the no, each with a few words if that helps.

When the question has no short likely answers, like a name, an address or a free-form \
description, or the reply does not ask the user anything, answer with nothing.";

/// What goes under a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Things to ask or do next, when Jev thinks the reply invites them.
    FollowUps,
    /// Answers to the question the reply ends on.
    Answers,
}

impl Kind {
    /// `Answers` when the last paragraph of `reply` asks something.
    pub fn of(reply: &str) -> Self {
        let last = reply.trim().rsplit("\n\n").next().unwrap_or_default();
        // A `?` inside a link is not a question.
        let asks = last.match_indices(['?', '？', '؟']).any(|(at, mark)| {
            last[at + mark.len()..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || matches!(c, '"' | ')' | '*' | '_' | '”'))
        });
        if asks { Self::Answers } else { Self::FollowUps }
    }
}

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
                EntryKind::Tool {
                    call_id, output, ..
                } => Some(ToolOutput {
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

impl Input<'_> {
    /// What the assistant knew, as a paragraph, or nothing without it.
    pub fn write_context(&self, f: &mut impl Write) -> fmt::Result {
        match self.context {
            Some(context) => writeln!(
                f,
                "What the assistant knew:\n{}\n",
                clip(context, MAX_CONTEXT_CHARS)
            ),
            None => Ok(()),
        }
    }

    /// What the turn's tools returned, as a paragraph, or nothing without any.
    pub fn write_results(&self, f: &mut impl Write) -> fmt::Result {
        if self.results.is_empty() {
            return Ok(());
        }
        write!(f, "What its tools returned:")?;
        for result in &self.results {
            write!(
                f,
                "\n- {}: {}",
                result.tool,
                clip(result.output, MAX_RESULT_CHARS)
            )?;
        }
        write!(f, "\n\n")
    }
}

impl fmt::Display for Input<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_context(f)?;
        if let Some(turn) = self.earlier {
            writeln!(f, "Earlier:\n{turn}\n")?;
        }
        self.write_results(f)?;
        // The reply goes last, next to the line saying to write in its
        // language, so the tool results' language is further away.
        write!(f, "Now:\n{}", self.latest)?;
        Ok(())
    }
}

/// The `suggest_replies` result to add under the reply, written only when Jev's `score` calls for one.
pub async fn write(
    generator: Generator<'_>,
    state: String,
    kind: Kind,
    reply: &str,
    score: Option<f64>,
) -> Option<String> {
    match kind {
        Kind::Answers => {
            let output = choice_output(items(generator, ANSWERS_SYSTEM, state, reply).await?);
            if output.is_none() {
                log::info!("no short answers to the reply's question");
            }
            output
        }
        Kind::FollowUps => {
            if score? < THRESHOLD {
                return None;
            }
            draft(generator, state, reply).await
        }
    }
}

/// Jev's estimate that the turn calls for follow-ups, asked on its own.
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

/// The follow-ups for `state`, whatever Jev thinks of the turn.
pub async fn draft(generator: Generator<'_>, state: String, reply: &str) -> Option<String> {
    let items = items(generator, SYSTEM, state, reply).await?;
    let output = replies_output(items.clone());
    if output.is_none() {
        log::warn!("fewer than two usable follow-ups: {items:?}");
    }
    output
}

/// The list items `generator` writes, unquoted, dropping any in another script than `reply`.
async fn items(
    generator: Generator<'_>,
    system: &str,
    state: String,
    reply: &str,
) -> Option<Vec<String>> {
    // Said last, next to the reply: in the system prompt alone the model
    // follows the place or the sources, like Japanese chips in Tokyo.
    let mut state =
        format!("{state}\n\nWrite every item in the language of the Assistant line above.");
    let mut retried = false;
    loop {
        let answer = match generator.text(system, state.clone()).await {
            Ok(answer) => answer,
            Err(error) => {
                log::warn!("unable to write follow-ups: {error}");
                return None;
            }
        };
        let (items, other): (Vec<String>, Vec<String>) = list_items(&answer)
            .into_iter()
            .map(|item| item.trim_matches(['"', '\'', '“', '”']).trim().to_owned())
            .partition(|item| same_script(item, reply));
        if !other.is_empty() {
            log::info!(
                "dropped {} follow-ups in another script than the reply",
                other.len()
            );
        }
        // Even said last, the place still wins now and then; one more try
        // with what went wrong spelled out usually lands.
        if !items.is_empty() || other.is_empty() || retried {
            return Some(items);
        }
        retried = true;
        let _ = write!(
            state,
            "\n\nYou wrote {other:?}, which is not the language of the Assistant line. \
             Write them again in the Assistant line's language."
        );
    }
}

/// A coarse writing system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Greek,
    Cyrillic,
    Hebrew,
    Arabic,
    /// Chinese, Japanese and Korean, which mix with each other.
    Cjk,
    Other,
}

const SCRIPTS: [Script; 7] = [
    Script::Latin,
    Script::Greek,
    Script::Cyrillic,
    Script::Hebrew,
    Script::Arabic,
    Script::Cjk,
    Script::Other,
];

fn script(c: char) -> Option<Script> {
    if !c.is_alphabetic() {
        return None;
    }
    Some(match u32::from(c) {
        0x0000..=0x024F | 0x1E00..=0x1EFF => Script::Latin,
        0x0370..=0x03FF => Script::Greek,
        0x0400..=0x052F => Script::Cyrillic,
        0x0590..=0x05FF => Script::Hebrew,
        0x0600..=0x06FF | 0x0750..=0x077F => Script::Arabic,
        0x3040..=0x30FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF
        | 0xFF66..=0xFF9F => Script::Cjk,
        _ => Script::Other,
    })
}

/// The script most of `text`'s letters are in, if it has any.
fn main_script(text: &str) -> Option<Script> {
    let mut counts = [0usize; SCRIPTS.len()];
    for script in text.chars().filter_map(script) {
        counts[script as usize] += 1;
    }
    let (most, count) = counts.iter().enumerate().max_by_key(|(_, count)| **count)?;
    (*count > 0).then_some(SCRIPTS[most])
}

/// Whether `item` is mostly in the same script as `reply`.
fn same_script(item: &str, reply: &str) -> bool {
    match (main_script(item), main_script(reply)) {
        (Some(item), Some(reply)) => item == reply,
        _ => true,
    }
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
    fn chips_in_another_script_than_the_reply_go() {
        let reply = "The museum opens at nine and is quieter before noon.";
        assert!(same_script("Remind me at 8:30", reply));
        assert!(!same_script("チケットの値段を調べて", reply));
        assert!(!same_script("Louvre の行き方を教えて", reply));
        let japanese = "美術館は九時に開き、昼前は空いています。";
        assert!(same_script("Louvre の行き方を教えて", japanese));
        assert!(!same_script("Find the ticket price", japanese));
        assert!(same_script("9:30", reply), "no letters says nothing");
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
    fn a_reply_ending_on_a_question_gets_answers() {
        assert_eq!(
            Kind::of("Sure.\n\nWhat time should I remind you? Say \"tomorrow 9am\"."),
            Kind::Answers
        );
        assert_eq!(Kind::of("Want me to book it?"), Kind::Answers);
        assert_eq!(Kind::of("**Lunch or dinner?**"), Kind::Answers);
        assert_eq!(Kind::of("要我帮你预订吗？"), Kind::Answers);
        assert_eq!(Kind::of("هل أحجز الطاولة؟"), Kind::Answers);
        assert_eq!(Kind::of("Which one?\n\nIt opens at nine."), Kind::FollowUps);
        assert_eq!(
            Kind::of("Here it is: https://example.com/search?q=beep"),
            Kind::FollowUps
        );
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
