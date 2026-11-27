//! pi's logs (`line`): the first line, `{"type":"session","id","cwd"}`,
//! names the session; each assistant `message` carries `usage` (`cost.total`
//! in US dollars when the provider priced it), `model` and `timestamp`.

use beep_proto::SessionId;
use super::{Line, Request, ledger::{Bucket, UNKNOWN_MODEL}};
use crate::{logs::found::unix_seconds, logs::prices::Tokens};
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<SessionId>,
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

/// What pi's session `id` spent so far, as its log in the sessions folder
/// `sessions` says: what the provider priced, else the model's list price.
/// Blocking: it reads the whole log.
pub(crate) fn session_cost(sessions: &Path, id: &SessionId) -> Option<f64> {
    let name = format!("_{}.jsonl", String::from(id));
    let log = fs::read_dir(sessions)
        .ok()?
        .flatten()
        .filter_map(|folder| fs::read_dir(folder.path()).ok())
        .flatten()
        .flatten()
        .map(|file| file.path())
        .find(|path| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file.ends_with(&name))
        })?;
    let text = fs::read_to_string(log).ok()?;
    let mut spent = Bucket::default();

    for request in text.lines().filter(|text| wanted(text)).filter_map(line) {
        if let Line::Request(request) = request {
            let model = request.model.as_deref().unwrap_or(UNKNOWN_MODEL);
            spent.add(model, &request.tokens, request.cost_usd);
        }
    }

    Some(spent.cost_usd)
}

/// Whether `line` may matter, before parsing it.
pub(crate) fn wanted(line: &str) -> bool {
    line.contains("\"usage\"") || line.contains("\"session\"")
}

/// What one line of a pi log says, if anything that counts.
pub(crate) fn line(text: &str) -> Option<Line> {
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
