//! Reads the current chat as saved, with every message the context may have dropped.

use super::memory::{arg, reply};
use crate::{
    memories::clip,
    store::{StoredBody, StoredMessage},
    user::User,
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// Characters of each message a read returns.
const MAX_READ_CHARS: usize = 3_000;
/// Characters around a match a search returns.
const SNIPPET_CHARS: usize = 300;
const MAX_MATCHES: usize = 20;
const DEFAULT_COUNT: usize = 20;
const MAX_COUNT: usize = 50;

/// Searches the current chat for a text.
pub(crate) struct ChatSearchTool {
    pub user: Arc<User>,
    pub chat: Uuid,
}

/// Reads a range of the current chat's messages.
pub(crate) struct ChatReadTool {
    pub user: Arc<User>,
    pub chat: Uuid,
}

/// A saved message as the model reads it.
struct Line<'a> {
    from: String,
    text: &'a str,
}

fn line(message: &StoredMessage) -> Option<Line<'_>> {
    let (from, text) = match &message.body {
        StoredBody::User { content, .. } => ("user".to_owned(), content.as_str()),
        StoredBody::Scheduled { label, .. } => ("routine".to_owned(), label.as_str()),
        StoredBody::Assistant { content } | StoredBody::Beep { content } => {
            ("assistant".to_owned(), content.as_str())
        }
        StoredBody::Tool { name, output, .. } => (format!("tool {name}"), output.as_str()),
        StoredBody::Member { name, content } => (format!("member {name}"), content.as_str()),
        StoredBody::Approval { .. } => return None,
    };
    Some(Line { from, text })
}

/// The chat's messages, oldest first; fails when it is not the user's.
async fn messages(
    user: &User,
    chat: Uuid,
    ctx: &ToolContext<'_>,
) -> Result<Result<Vec<StoredMessage>, ToolResult>> {
    Ok(match user.session_messages(chat).await? {
        Some(messages) => Ok(messages),
        None => Err(ToolResult::failure(
            ctx.tool_call_id,
            "This chat has no saved messages to read.",
        )),
    })
}

/// About `SNIPPET_CHARS` of `text` around the match at byte `at` of its lowercase `lower`.
fn snippet(text: &str, lower: &str, at: usize) -> String {
    let start = lower[..at]
        .chars()
        .count()
        .saturating_sub(SNIPPET_CHARS / 3);
    text.chars().skip(start).take(SNIPPET_CHARS).collect()
}

#[async_trait]
impl Tool for ChatSearchTool {
    fn name(&self) -> &str {
        "chat_search"
    }
    fn description(&self) -> &str {
        "Search every saved message of this chat, tool output included, for a text, ignoring case. Your context may have summarized older turns or dropped old tool output; use this to find what was said or returned there. Then read around a match with chat_read."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string","description":"The text to look for."}
        },"required":["query"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let query = arg(&ctx.args, "query")?.trim().to_lowercase();
        if query.is_empty() {
            return Ok(ToolResult::failure(ctx.tool_call_id, "The query is empty."));
        }
        let messages = match messages(&self.user, self.chat, &ctx).await? {
            Ok(messages) => messages,
            Err(failure) => return Ok(failure),
        };
        let matches: Vec<Value> = messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let line = line(message)?;
                let lower = line.text.to_lowercase();
                let at = lower.find(&query)?;
                Some(json!({
                    "index": index,
                    "time": message.sent_at.to_rfc3339(),
                    "from": line.from,
                    "snippet": snippet(line.text, &lower, at),
                }))
            })
            .take(MAX_MATCHES)
            .collect();
        reply(
            &ctx,
            json!({"messages": messages.len(), "matches": matches}),
        )
    }
}

#[async_trait]
impl Tool for ChatReadTool {
    fn name(&self) -> &str {
        "chat_read"
    }
    fn description(&self) -> &str {
        "Read this chat's saved messages in order, tool output included, starting at an index (0 is the first message). Your context may have summarized older turns or dropped old tool output; use this to see them as they were. Long messages are cut."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "start":{"type":"integer","minimum":0,"description":"Index of the first message to read. Defaults to 0."},
            "count":{"type":"integer","minimum":1,"maximum":MAX_COUNT,"description":"How many messages to read. Defaults to 20."}
        },"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let start = ctx.args.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let count = ctx
            .args
            .get("count")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_COUNT, |count| (count as usize).clamp(1, MAX_COUNT));
        let messages = match messages(&self.user, self.chat, &ctx).await? {
            Ok(messages) => messages,
            Err(failure) => return Ok(failure),
        };
        let read: Vec<Value> = messages
            .iter()
            .enumerate()
            .skip(start)
            .take(count)
            .filter_map(|(index, message)| {
                let line = line(message)?;
                Some(json!({
                    "index": index,
                    "time": message.sent_at.to_rfc3339(),
                    "from": line.from,
                    "text": clip(line.text, MAX_READ_CHARS),
                }))
            })
            .collect();
        reply(&ctx, json!({"messages": messages.len(), "read": read}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_starts_a_little_before_the_match() {
        let text = format!("{}Needle here", "a".repeat(500));
        let lower = text.to_lowercase();
        let at = lower.find("needle").unwrap();
        let snippet = snippet(&text, &lower, at);
        assert!(snippet.starts_with(&"a".repeat(SNIPPET_CHARS / 3)));
        assert!(snippet.contains("Needle here"));
    }
}
