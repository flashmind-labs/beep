//! Codex's logs: the `session_meta` line names the session (`payload.id`)
//! and its folder, each `turn_context` the model the turn runs, and each
//! `event_msg` of type `token_count` the last request's tokens
//! (`info.last_token_usage`) and the running total. Input counts cached
//! input; output counts reasoning. Codex repeats a count when only its
//! rate limits changed, which the running total tells.

use super::{Line, Request};
use crate::{agents::prices::Tokens, claude::unix_seconds};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    payload: Option<Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Count {
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

/// Whether `line` may matter, before parsing it.
pub(super) fn wanted(line: &str) -> bool {
    line.contains("\"token_count\"")
        || line.contains("\"turn_context\"")
        || line.contains("\"session_meta\"")
}

/// What one line of a Codex log says, if anything that counts.
pub(super) fn line(text: &str) -> Option<Line> {
    let entry: Entry = serde_json::from_str(text).ok()?;
    let payload = entry.payload?;
    let text_of = |key: &str| payload.get(key).and_then(Value::as_str).map(str::to_owned);
    match entry.kind.as_deref()? {
        "session_meta" => Some(Line::Session {
            id: text_of("id")?,
            cwd: text_of("cwd").unwrap_or_default(),
        }),
        "turn_context" => text_of("model").map(Line::Model),
        "event_msg" if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
            let info = payload.get("info")?;
            let count = |key: &str| {
                info.get(key)
                    .and_then(|count| Count::deserialize(count).ok())
            };
            let last = count("last_token_usage")?;
            let total = count("total_token_usage").map(|total| total.total_tokens);
            let cached = last.cached_input_tokens.min(last.input_tokens);
            let written = last
                .cache_write_input_tokens
                .min(last.input_tokens - cached);
            let tokens = Tokens {
                input: last.input_tokens - cached - written,
                output: last.output_tokens,
                cache_read: cached,
                cache_write: written,
                cache_write_long: 0,
            };
            if tokens.total() == 0 {
                return None;
            }
            Some(Line::Request(Request {
                at: entry.timestamp.as_deref().and_then(unix_seconds)?,
                tokens,
                total,
                ..Request::default()
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_gives_its_session_model_and_counts() {
        let meta = r#"{"timestamp":"2026-11-02T10:00:00.000Z","type":"session_meta","payload":{"id":"0199a1b2-3c4d-7e5f-8a9b-0c1d2e3f4a5b","cwd":"/srv/project","source":"cli"}}"#;
        assert!(wanted(meta));
        assert!(matches!(
            line(meta),
            Some(Line::Session { id, cwd }) if id.starts_with("0199") && cwd == "/srv/project"
        ));
        let turn = r#"{"timestamp":"2026-11-02T10:00:01.000Z","type":"turn_context","payload":{"cwd":"/srv/project","model":"gpt-6.1-sol"}}"#;
        assert!(matches!(line(turn), Some(Line::Model(model)) if model == "gpt-6.1-sol"));
        let count = r#"{"timestamp":"2026-11-02T10:15:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":28949,"cached_input_tokens":28032,"output_tokens":99,"total_tokens":29048},"last_token_usage":{"input_tokens":28949,"cached_input_tokens":28032,"output_tokens":99,"total_tokens":29048}}}}"#;
        let Some(Line::Request(request)) = line(count) else {
            panic!("a request");
        };
        assert_eq!(request.tokens.input, 917);
        assert_eq!(request.tokens.cache_read, 28032);
        assert_eq!(request.tokens.output, 99);
        assert_eq!(request.total, Some(29048));
        assert_eq!(request.at, 1_793_614_500);
    }

    #[test]
    fn a_count_without_info_gives_nothing() {
        let empty = r#"{"timestamp":"2026-11-02T10:15:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#;
        assert!(line(empty).is_none());
    }
}
