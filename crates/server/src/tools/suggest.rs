//! The suggestions card the phone shows as tappable replies under a message.
//! The server writes them after the turn (see `followups`), saved as a
//! `suggest_replies` tool result so older chats and phones read them the same.
use serde::Serialize;

/// Longer replies are dropped rather than cut, so no chip reads half-written.
const MAX_REPLY_CHARS: usize = 50;
const MIN_REPLIES: usize = 2;
const MAX_REPLIES: usize = 3;
/// A question can name a few more options than a follow-up row holds.
const MAX_CHOICES: usize = 4;
pub(crate) const NAME: &str = "suggest_replies";
pub(crate) const SUGGESTING: &str = "Suggesting replies";

/// What the phone receives in the tool result.
#[derive(Debug, PartialEq, Serialize)]
struct Suggestions {
    #[serde(rename = "type")]
    card: &'static str,
    kind: Kind,
    replies: Vec<String>,
}

/// `replies` for follow-ups, `choice` for answers to a question the reply
/// asked, which phones draw as full-width buttons.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Replies,
    Choice,
}

/// `text` with its whitespace collapsed, or `None` when it is empty or
/// longer than `max` characters.
fn fit(text: &str, max: usize) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty() && text.chars().count() <= max).then_some(text)
}

/// `replies` that fit, without duplicates, at most `max`.
fn suggestions(replies: Vec<String>, kind: Kind, max: usize) -> Option<Suggestions> {
    let mut kept: Vec<String> = Vec::new();
    for reply in &replies {
        let Some(reply) = fit(reply, MAX_REPLY_CHARS) else {
            continue;
        };
        if !kept.iter().any(|r| r.eq_ignore_ascii_case(&reply)) {
            kept.push(reply);
        }
    }
    kept.truncate(max);
    (kept.len() >= MIN_REPLIES).then_some(Suggestions {
        card: "suggestions",
        kind,
        replies: kept,
    })
}

/// The tool result for `replies`, or `None` when fewer than two are usable.
pub(crate) fn replies_output(replies: Vec<String>) -> Option<String> {
    serde_json::to_string(&suggestions(replies, Kind::Replies, MAX_REPLIES)?).ok()
}

/// The tool result for answers to the reply's question, or `None` when fewer
/// than two are usable.
pub(crate) fn choice_output(answers: Vec<String>) -> Option<String> {
    serde_json::to_string(&suggestions(answers, Kind::Choice, MAX_CHOICES)?).ok()
}
