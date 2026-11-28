//! Child notices as Markdown for the parent's conversation.

use crate::markdown;
use beep_node::tags::{Part, parts};

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

/// The footer of a child that can be continued; older nodes named the tool
/// `send_to_agent`.
const REPLY: [&str; 2] = [
    "Reply with agent send to continue it.",
    "Reply with send_to_agent to continue it.",
];

/// Remove only the wrapper Beep writes, leaving the child's own text intact.
fn body<'a>(id: &str, state: &str, text: &'a str) -> &'a str {
    let (status, footers): (&str, &[&str]) = match state {
        "finished" => ("finished", &REPLY),
        "asking" => (
            "is waiting on the user",
            &["Only the user can answer this."],
        ),
        "stopped" => ("was stopped by the user", &REPLY),
        "closed" => ("was stopped by the user", &["Its session ended."]),
        _ => return text,
    };
    let prefix = format!("[child {id} {status}]");
    let Some(text) = text.strip_prefix(&prefix) else {
        return text;
    };
    let text = text.strip_prefix(' ').unwrap_or(text);
    footers
        .iter()
        .find_map(|footer| text.strip_suffix(&format!("\n{footer}")))
        .unwrap_or(text)
        .trim_matches('\n')
}

#[cfg(test)]
#[path = "../tests/agents_child_reply.rs"]
mod tests;
