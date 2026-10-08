//! The suggestions card the phone shows as tappable replies under a message.
//! The server writes them after the turn (see `followups`), saved as a
//! `suggest_replies` tool result so older chats and phones read them the same.
use serde::Serialize;

/// Longer replies are dropped rather than cut, so no chip reads half-written.
const MAX_REPLY_CHARS: usize = 50;
const MIN_REPLIES: usize = 2;
const MAX_REPLIES: usize = 3;
pub(crate) const NAME: &str = "suggest_replies";
pub(crate) const SUGGESTING: &str = "Suggesting replies";

/// What the phone receives in the tool result.
#[derive(Debug, PartialEq, Serialize)]
struct Suggestions {
    #[serde(rename = "type")]
    card: &'static str,
    /// Always `replies`; phones also know `choice`, from when the model
    /// offered options itself.
    kind: &'static str,
    replies: Vec<String>,
}

/// `text` with its whitespace collapsed, or `None` when it is empty or
/// longer than `max` characters.
fn fit(text: &str, max: usize) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty() && text.chars().count() <= max).then_some(text)
}

/// `replies` that fit, without duplicates, at most three.
fn suggestions(replies: Vec<String>) -> Option<Suggestions> {
    let mut kept: Vec<String> = Vec::new();
    for reply in &replies {
        let Some(reply) = fit(reply, MAX_REPLY_CHARS) else {
            continue;
        };
        if !kept.iter().any(|r| r.eq_ignore_ascii_case(&reply)) {
            kept.push(reply);
        }
    }
    kept.truncate(MAX_REPLIES);
    (kept.len() >= MIN_REPLIES).then_some(Suggestions {
        card: "suggestions",
        kind: "replies",
        replies: kept,
    })
}

/// The tool result for `replies`, or `None` when fewer than two are usable.
pub(crate) fn replies_output(replies: Vec<String>) -> Option<String> {
    serde_json::to_string(&suggestions(replies)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn output(replies: &[&str]) -> Option<Value> {
        let replies = replies.iter().map(|r| r.to_string()).collect();
        replies_output(replies).map(|out| serde_json::from_str(&out).unwrap())
    }

    #[test]
    fn output_shape() {
        assert_eq!(
            output(&["Yes please", "  No thanks ", "yes PLEASE", ""]),
            Some(
                json!({"type": "suggestions", "kind": "replies", "replies": ["Yes please", "No thanks"]})
            )
        );
        assert_eq!(
            output(&["a", "b", "c", "d"]).unwrap()["replies"],
            json!(["a", "b", "c"])
        );
    }

    #[test]
    fn long_replies_are_dropped_and_too_few_rejected() {
        let long = "Book a table for two at the Italian place tonight at eight, by the window";
        assert!(
            output(&[long, "Not now", "Later"]).unwrap()["replies"] == json!(["Not now", "Later"])
        );
        assert!(output(&[long, "Not now"]).is_none());
        assert!(output(&["Only one"]).is_none());
    }
}
