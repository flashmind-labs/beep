//! Follow-up replies for turns where the model offered none. Jev decides
//! whether the reply invites one, then a plain completion writes them.
use crate::{
    decision::{Criteria, DecisionTool},
    memories::Turn,
    starters::{Generator, unfence},
    tools::{SUGGEST_REPLIES, replies_output},
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use flashmind::core::{Conversation, EntryKind};

/// How likely a follow-up has to be useful before one is written.
const THRESHOLD: f64 = 0.5;

const QUESTION: &str = "Would the user likely want tappable follow-ups under this reply: a question to dig deeper into the topic, or something the assistant could do next with what it just said?";

const CRITERIA: Criteria = Criteria {
    yes: "The reply explains a topic, answers a factual question, recommends or compares things, or finds something that leads to an action like a reminder, directions, a calendar event or a search.",
    no: "Small talk, thanks or goodbyes, a finished task with nothing left to do, or a reply that already asks the user a question.",
};

const SYSTEM: &str = "You write follow-ups shown as tappable chips under an assistant's reply. \
Return only a JSON array of 2 to 4 strings, no prose. Each string is the message the user would \
send next, written as the user, at most 40 characters. Mix questions that dig deeper into the \
topic with things the assistant could do next: search the web, set a reminder, add a calendar \
event, show a place on the map, check mail. Do not repeat what the reply already says. \
Write in the user's language.";

/// Whether this turn already shows replies under its message.
pub fn offered(conversation: &Conversation, since: DateTime<Utc>) -> bool {
    conversation
        .entries()
        .iter()
        .filter(|entry| entry.timestamp >= since)
        .any(|entry| {
            matches!(&entry.kind, EntryKind::Assistant { tool_calls: Some(calls), .. }
                if calls.iter().any(|call| call.name == SUGGEST_REPLIES))
        })
}

/// The `suggest_replies` result to add under the reply, or `None` when the
/// turn does not call for one or writing it failed.
pub async fn write(
    decision: &DecisionTool,
    generator: Generator<'_>,
    state: String,
) -> Option<String> {
    let probability = match decision.probability_with(&state, QUESTION, &CRITERIA).await {
        Ok(probability) => probability,
        Err(error) => {
            log::warn!("follow-up score failed: {error}");
            return None;
        }
    };
    log::info!("follow-up score {probability:.2}");
    if probability < THRESHOLD {
        return None;
    }
    match generator
        .text(SYSTEM, state)
        .await
        .and_then(|answer| parse(&answer))
    {
        Ok(replies) => replies_output(replies),
        Err(error) => {
            log::warn!("unable to write follow-ups: {error}");
            None
        }
    }
}

/// What both Jev and the writer read: the last earlier turn for context,
/// then the latest exchange.
pub fn render(earlier: &[Turn], latest: &Turn) -> String {
    match earlier.last() {
        Some(turn) => format!("Earlier:\n{turn}\n\nNow:\n{latest}"),
        None => latest.to_string(),
    }
}

/// The writer's replies; `replies_output` drops duplicates and checks the count.
/// Models sometimes wrap the array in a sentence, so the outermost brackets
/// are taken when the whole answer does not parse.
fn parse(answer: &str) -> Result<Vec<String>> {
    let text = unfence(answer);
    if let Ok(replies) = serde_json::from_str(text) {
        return Ok(replies);
    }
    let array = text
        .find('[')
        .zip(text.rfind(']'))
        .filter(|(start, end)| start < end)
        .map(|(start, end)| &text[start..=end]);
    match array {
        Some(array) => Ok(serde_json::from_str(array)?),
        None => bail!("no JSON array in a {}-character answer", answer.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use flashmind::core::ConversationEntry;
    use flashmind::types::ToolCall;

    #[test]
    fn parses_fenced_arrays() {
        let replies = parse("```json\n[\"How are they priced?\", \"Show me one\"]\n```").unwrap();
        assert_eq!(replies, ["How are they priced?", "Show me one"]);
        let replies = parse("Here are some:\n[\"Show me one\", \"Who built it?\"]").unwrap();
        assert_eq!(replies, ["Show me one", "Who built it?"]);
        assert!(parse("Sure! Here you go").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn finds_replies_offered_this_turn() {
        let since = Utc::now();
        let mut conversation = Conversation::new();
        let add = |conversation: &mut Conversation, name: &str, at: DateTime<Utc>| {
            let call = ToolCall {
                id: name.to_owned(),
                name: name.to_owned(),
                arguments: serde_json::json!({}),
            };
            let mut entry = ConversationEntry::assistant_with_tool_calls("", vec![call], None);
            entry.timestamp = at;
            conversation.add(entry);
        };
        add(
            &mut conversation,
            SUGGEST_REPLIES,
            since - TimeDelta::seconds(1),
        );
        add(&mut conversation, "web_search", since);
        assert!(!offered(&conversation, since));
        add(&mut conversation, SUGGEST_REPLIES, since);
        assert!(offered(&conversation, since));
    }
}
