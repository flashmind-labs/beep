//! What Hermes's event frames become for the session: `session/update`
//! objects.

use serde_json::{Value, json};

/// The `session/update` objects for the `params` of a Hermes event frame
/// (`type` and `payload`). Events with nothing to show give none; so does
/// `thinking.delta`, which is Hermes's spinner text, not reasoning.
pub(super) fn update(event: &Value) -> Vec<Value> {
    let payload = &event["payload"];
    let update = match event["type"].as_str() {
        Some("message.delta") => chunk("agent_message_chunk", payload["text"].as_str()),
        Some("reasoning.delta") => chunk("agent_thought_chunk", payload["text"].as_str()),
        Some("tool.start") => {
            let name = payload["name"].as_str().unwrap_or("tool");
            Some(json!({
                "sessionUpdate": "tool_call",
                "toolCallId": payload["tool_id"],
                "title": name,
                "kind": if name == "terminal" { "execute" } else { "other" },
                "status": "in_progress",
                "rawInput": payload["args"],
            }))
        }
        Some("tool.progress") => Some(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": payload["tool_id"],
            "status": "in_progress",
            "content": text_content(payload["text"].as_str().or(payload["preview"].as_str())),
        })),
        Some("tool.complete") => {
            let result = &payload["result"];
            let failed = !result["error"].is_null() || result["exit_code"].as_i64().is_some_and(|code| code != 0);
            let shown = result["output"].as_str().or(result["error"].as_str());
            Some(json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": payload["tool_id"],
                "status": if failed { "failed" } else { "completed" },
                "rawOutput": result,
                "content": text_content(shown),
            }))
        }
        Some("session.title") => Some(json!({"sessionUpdate": "session_info_update", "title": payload["title"]})),
        _ => None,
    };

    update.into_iter().collect()
}

/// Whether the event ends the prompt turn.
pub(super) fn finished(event: &Value) -> bool {
    event["type"] == "message.complete"
}

fn chunk(kind: &str, text: Option<&str>) -> Option<Value> {
    let text = text.filter(|text| !text.is_empty())?;
    Some(json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}}))
}

fn text_content(text: Option<&str>) -> Vec<Value> {
    text.filter(|text| !text.is_empty())
        .map(|text| json!({"type": "content", "content": {"type": "text", "text": text}}))
        .into_iter()
        .collect()
}
