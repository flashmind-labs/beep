//! Codex's logs (`line`): `session_meta` names the session, `turn_context`
//! the model, and each `token_count` event the last request's tokens.

use super::{Line, Request};
use crate::{logs::found::unix_seconds, logs::prices::Tokens};
use beep_proto::SessionId;
use beep_proto::node::usage::Billing;
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
pub(crate) fn wanted(line: &str) -> bool {
    line.contains("\"token_count\"")
        || line.contains("\"turn_context\"")
        || line.contains("\"session_meta\"")
}

/// What one line of a Codex log says, if anything that counts. Input
/// counts cached input; output counts reasoning. Codex repeats a count when
/// only its rate limits changed, which the running total tells.
pub(crate) fn line(text: &str) -> Option<Line> {
    let entry: Entry = serde_json::from_str(text).ok()?;
    let payload = entry.payload?;
    let text_of = |key: &str| payload.get(key).and_then(Value::as_str).map(str::to_owned);

    match entry.kind.as_deref()? {
        "session_meta" => Some(Line::Session {
            id: text_of("id").map(Into::<SessionId>::into)?,
            cwd: text_of("cwd").unwrap_or_default(),
            provider: text_of("model_provider"),
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
            // Only a ChatGPT sign-in has plan limits.
            let plan = payload
                .get("rate_limits")
                .and_then(|limits| limits.get("plan_type"))
                .and_then(Value::as_str)
                .is_some_and(|plan| !plan.is_empty());
            Some(Line::Request(Request {
                at: entry.timestamp.as_deref().and_then(unix_seconds)?,
                tokens,
                total,
                billing: if plan {
                    Billing::Subscription
                } else {
                    Billing::Unknown
                },
                ..Request::default()
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../tests/logs_spend_codex.rs"]
mod tests;
