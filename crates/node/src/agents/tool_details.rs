//! Bounded tool arguments and results, separate from the agent's label.

use super::live::cut;
use serde_json::{Map, Value};

const MAX_DETAIL: usize = 8 * 1024;

pub(super) fn json(value: Option<&Value>) -> (Option<String>, bool) {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return (None, false);
    };
    let text = value.to_string();
    let truncated = text.len() > MAX_DETAIL;
    (Some(cut(&text, MAX_DETAIL)), truncated)
}

/// A name reported by the harness, never inferred from its prose title.
pub(super) fn name(meta: Option<&Map<String, Value>>) -> Option<String> {
    meta?
        .get("claudeCode")?
        .get("toolName")?
        .as_str()
        .filter(|name| !name.is_empty())
        .map(|name| cut(name, 240))
}

#[cfg(test)]
#[path = "../tests/agents_tool_details.rs"]
mod tests;
