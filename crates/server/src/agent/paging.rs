//! Tells the model when a tool result is only the first page.

use flashmind::types::{ContentPart, tool::ToolResult};
use serde_json::Value;

/// Put before the result, so cutting a long output cannot drop it.
const MORE: &str = "This is not every result: the tool says there are more pages. Before \
giving a count, a total or saying this is all of them, or when acting on all of them, \
fetch the next page with the cursor or page token below. Otherwise tell the user the list \
is partial.\n";

/// How deep paging fields are looked for (Composio nests them under `data`).
const MAX_DEPTH: usize = 3;

/// Replaces a successful result's text with `rewrite(text)`, keeping its media parts.
pub fn rewrite_text(result: &mut ToolResult, rewrite: impl FnOnce(String) -> Option<String>) {
    let ToolResult::Success { content, .. } = result else {
        return;
    };
    if !content
        .iter()
        .any(|part| matches!(part, ContentPart::Text { .. }))
    {
        return;
    }
    let text: String = content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let Some(new) = rewrite(text.clone()) else {
        return;
    };
    if new == text {
        return;
    }
    content.retain(|part| !matches!(part, ContentPart::Text { .. }));
    content.insert(0, ContentPart::Text { text: new });
}

/// `output` with a note in front when it says more results are left.
pub fn mark(output: String) -> String {
    let more = output
        .find(['{', '['])
        .and_then(|start| serde_json::from_str::<Value>(&output[start..]).ok())
        .is_some_and(|value| has_more(&value, 0));
    if more {
        format!("{MORE}{output}")
    } else {
        output
    }
}

/// Whether `value` says there is another page, in any provider's style.
fn has_more(value: &Value, depth: usize) -> bool {
    let Value::Object(map) = value else {
        return false;
    };
    let set = |key: &str| match map.get(key) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => !text.is_empty(),
        _ => false,
    };
    let flagged = ["hasMore", "has_more", "has_next_page"]
        .into_iter()
        .any(set);
    let linked = [
        "nextPageToken",
        "next_page_token",
        "nextCursor",
        "next_cursor",
        "@odata.nextLink",
    ]
    .into_iter()
    .any(set);
    let paged = map.get("paging").is_some_and(|paging| {
        let number = |key| paging.get(key).and_then(Value::as_u64);
        matches!((number("page"), number("pages")), (Some(page), Some(pages)) if page < pages)
    });
    if flagged || linked || paged {
        return true;
    }
    depth < MAX_DEPTH && map.values().any(|value| has_more(value, depth + 1))
}

#[cfg(test)]
#[path = "../tests/agent_paging.rs"]
mod tests;
