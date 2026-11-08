//! Child notices as Markdown for the parent's conversation.

use crate::chat::markdown;
use beep_node::agents::tags::{Part, parts};

/// HTML for each child part, in the order it appears in the entry.
pub(super) fn replies(text: &str) -> Vec<String> {
    parts(text)
        .into_iter()
        .filter_map(|part| match part {
            Part::Child {
                id, state, text, ..
            } => Some(markdown::untrusted_html(body(&id, &state, text))),
            _ => None,
        })
        .collect()
}

/// Remove only the wrapper Beep writes, leaving the child's own text intact.
fn body<'a>(id: &str, state: &str, text: &'a str) -> &'a str {
    let (status, footer) = match state {
        "finished" => ("finished", "Reply with send_to_agent to continue it."),
        "asking" => ("is waiting on the user", "Only the user can answer this."),
        "stopped" => (
            "was stopped by the user",
            "Reply with send_to_agent to continue it.",
        ),
        "closed" => ("was stopped by the user", "Its session ended."),
        _ => return text,
    };
    let prefix = format!("[child {id} {status}]");
    let Some(text) = text.strip_prefix(&prefix) else {
        return text;
    };
    let text = text.strip_prefix(' ').unwrap_or(text);
    let suffix = format!("\n{footer}");
    text.strip_suffix(&suffix)
        .unwrap_or(text)
        .trim_matches('\n')
}

#[cfg(test)]
#[path = "../tests/agents_child_reply.rs"]
mod tests;
