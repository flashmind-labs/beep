//! pi's logs: the first line, `{"type":"session","id","cwd"}`, names the
//! session; each assistant `message` carries `usage` (`input` read fresh,
//! `output`, `cacheRead`, `cacheWrite`, and `cost.total` in US dollars when
//! the provider priced it), `model` and a `timestamp`.

use super::{Line, Request};
use crate::{agents::prices::Tokens, claude::unix_seconds};
use serde::Deserialize;

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    role: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
    /// Unix milliseconds.
    timestamp: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cost: Option<Cost>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Cost {
    total: f64,
}

/// Whether `line` may matter, before parsing it.
pub(super) fn wanted(line: &str) -> bool {
    line.contains("\"usage\"") || line.contains("\"session\"")
}

/// What one line of a pi log says, if anything that counts.
pub(super) fn line(text: &str) -> Option<Line> {
    let entry: Entry = serde_json::from_str(text).ok()?;
    match entry.kind.as_deref()? {
        "session" => Some(Line::Session {
            id: entry.id?,
            cwd: entry.cwd.unwrap_or_default(),
            provider: None,
        }),
        "message" => {
            let message = entry.message?;
            if message.role.as_deref() != Some("assistant") {
                return None;
            }
            let usage = message.usage?;
            let tokens = Tokens {
                input: usage.input,
                output: usage.output,
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
                cache_write_long: 0,
            };
            if tokens.total() == 0 {
                return None;
            }
            let at = entry
                .timestamp
                .as_deref()
                .and_then(unix_seconds)
                .or_else(|| message.timestamp.map(|ms| ms / 1000))?;
            Some(Line::Request(Request {
                at,
                model: message.model,
                tokens,
                cost_usd: usage.cost.map(|cost| cost.total),
                provider: message.provider,
                ..Request::default()
            }))
        }
        _ => None,
    }
}
