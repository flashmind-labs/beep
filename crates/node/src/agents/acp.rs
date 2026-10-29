//! The little of JSON-RPC 2.0 that ACP needs: one message per line on the
//! adapter's stdio.

use serde_json::{Value, json};

/// The ACP version this client speaks.
pub(super) const PROTOCOL_VERSION: u64 = 1;

const METHOD_NOT_FOUND: i64 = -32601;

/// A line the agent sent.
#[derive(Debug, PartialEq)]
pub(super) enum Incoming {
    /// The answer to one of our requests.
    Response {
        id: u64,
        result: Result<Value, String>,
    },
    /// The agent asks us something; `id` goes back in the answer as is.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
}

/// Reads one line, `None` when it is not a JSON-RPC message.
pub(super) fn parse(line: &str) -> Option<Incoming> {
    let mut message: Value = serde_json::from_str(line.trim()).ok()?;
    let params = message.get_mut("params").map(Value::take);
    match message.get("method").and_then(Value::as_str) {
        Some(method) => {
            let method = method.to_owned();
            let params = params.unwrap_or(Value::Null);
            Some(match message.get_mut("id").map(Value::take) {
                Some(id) if !id.is_null() => Incoming::Request { id, method, params },
                _ => Incoming::Notification { method, params },
            })
        }
        None => {
            let id = message.get("id")?.as_u64()?;
            let result = match message.get_mut("error") {
                Some(error) => Err(error_text(error)),
                None => Ok(message
                    .get_mut("result")
                    .map(Value::take)
                    .unwrap_or_default()),
            };
            Some(Incoming::Response { id, result })
        }
    }
}

fn error_text(error: &Value) -> String {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    match error.get("data") {
        Some(Value::String(data)) => format!("{message}: {data}"),
        Some(data) if !data.is_null() => format!("{message}: {data}"),
        _ => message.to_owned(),
    }
}

pub(super) fn request(id: u64, method: &str, params: Value) -> String {
    line(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
}

pub(super) fn notification(method: &str, params: Value) -> String {
    line(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
}

pub(super) fn response(id: &Value, result: Value) -> String {
    line(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Refuses a request this client does not serve.
pub(super) fn not_supported(id: &Value, method: &str) -> String {
    line(json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": METHOD_NOT_FOUND, "message": format!("{method} is not supported") }
    }))
}

fn line(message: Value) -> String {
    let mut text = message.to_string();
    text.push('\n');
    text
}

/// What `initialize` sends: this client reads and writes no files and runs
/// no terminals for the agent, so it does that work itself.
pub(super) fn initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": { "readTextFile": false, "writeTextFile": false },
            "terminal": false
        },
        "clientInfo": { "name": "beep-node", "version": env!("CARGO_PKG_VERSION") }
    })
}

pub(super) fn new_session(cwd: &str) -> Value {
    json!({ "cwd": cwd, "mcpServers": [] })
}

/// The params of `session/resume` or `session/load` for `session`.
pub(super) fn resume_session(session: &str, cwd: &str) -> Value {
    json!({ "sessionId": session, "cwd": cwd, "mcpServers": [] })
}

/// How the agent the `initialize` answer describes resumes a session:
/// `session/resume`, which replays nothing, or else `session/load`, which
/// replays the history; `None` when it does neither.
pub(super) fn resume_method(initialized: &Value) -> Option<&'static str> {
    let capabilities = &initialized["agentCapabilities"];
    if capabilities["sessionCapabilities"]["resume"].is_object() {
        Some("session/resume")
    } else if capabilities["loadSession"].as_bool() == Some(true) {
        Some("session/load")
    } else {
        None
    }
}

/// Claude Code's mode that never asks permission, as
/// `claude --dangerously-skip-permissions` runs.
pub(super) const BYPASS_MODE: &str = "bypassPermissions";

pub(super) fn set_mode(session: &str, mode: &str) -> Value {
    json!({ "sessionId": session, "modeId": mode })
}

pub(super) fn prompt(session: &str, text: &str) -> Value {
    json!({ "sessionId": session, "prompt": [{ "type": "text", "text": text }] })
}

pub(super) fn cancel(session: &str) -> Value {
    json!({ "sessionId": session })
}

pub(super) fn selected(option: &str) -> Value {
    json!({ "outcome": { "outcome": "selected", "optionId": option } })
}

pub(super) fn cancelled() -> Value {
    json!({ "outcome": { "outcome": "cancelled" } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_as_responses_requests_and_notifications() {
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#),
            Some(Incoming::Response {
                id: 3,
                result: Ok(json!({"stopReason": "end_turn"}))
            })
        );
        assert_eq!(
            parse(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Authentication required"}}"#
            ),
            Some(Incoming::Response {
                id: 1,
                result: Err("Authentication required".into())
            })
        );
        assert_eq!(
            parse(
                r#"{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"x":1}}"#
            ),
            Some(Incoming::Request {
                id: json!("p1"),
                method: "session/request_permission".into(),
                params: json!({"x": 1})
            })
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#),
            Some(Incoming::Notification {
                method: "session/update".into(),
                params: json!({})
            })
        );
        assert_eq!(parse("npm warn something"), None);
    }

    #[test]
    fn resume_prefers_session_resume_to_load() {
        let both = json!({"agentCapabilities": {"loadSession": true, "sessionCapabilities": {"resume": {}}}});
        assert_eq!(resume_method(&both), Some("session/resume"));
        let load = json!({"agentCapabilities": {"loadSession": true}});
        assert_eq!(resume_method(&load), Some("session/load"));
        assert_eq!(resume_method(&json!({"agentCapabilities": {}})), None);
    }

    #[test]
    fn messages_are_one_line_each() {
        let text = request(7, "session/prompt", prompt("s", "two\nlines"));
        assert!(text.ends_with('\n'));
        assert_eq!(text.matches('\n').count(), 1);
    }
}
