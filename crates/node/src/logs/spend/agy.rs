//! What Antigravity sessions spent (`conversation`), from the SQLite
//! database it keeps of each conversation, read whole again whenever one
//! changes. A subagent's conversation counts toward the session that
//! started it.

use super::{
    billing::Accounts,
    ledger::{BucketKey, Part, UNKNOWN_MODEL},
};
use crate::logs::found::{
    Agent,
    agy::{Time, Trajectory},
    open_database,
};
use crate::logs::prices::Tokens;
use beep_proto::{ModelId, SessionId, node::usage::Billing};
use prost::Message;
use rusqlite::OptionalExtension;
use std::{collections::BTreeMap, path::Path};

/// The most model requests one conversation gives in a pass.
const MAX_ROWS: i64 = 50_000;

const GENERATIONS: &str = "SELECT data FROM gen_metadata ORDER BY idx LIMIT ?1";
const STEP: &str = "SELECT metadata FROM steps WHERE idx = ?1";
const TRAJECTORY: &str = "SELECT data FROM trajectory_metadata_blob LIMIT 1";

/// What the conversation in `database` spent from `since` (Unix seconds) on,
/// under its root session: the file's name, or the session that started it.
pub(crate) fn conversation(
    database: &Path,
    since: u64,
    accounts: &mut Accounts,
) -> rusqlite::Result<BTreeMap<SessionId, Part>> {
    let conn = open_database(database)?;
    let trajectory = conn
        .query_row(TRAJECTORY, [], |row| row.get::<_, Vec<u8>>(0))
        .optional()?
        .and_then(|data| Trajectory::decode(data.as_slice()).ok())
        .unwrap_or_default();
    let cwd = trajectory
        .workspaces
        .first()
        .and_then(|workspace| url::Url::parse(&workspace.uri).ok()?.to_file_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let started = trajectory.created.map_or(0, |time| time.seconds);
    let own = database
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let billing = accounts.resolve(Agent::Agy, Billing::Unknown, None, Some(cwd.as_str()));
    let mut steps = conn.prepare(STEP)?;
    let mut generations = conn.prepare(GENERATIONS)?;
    let mut rows = generations.query([MAX_ROWS])?;
    let mut parts: BTreeMap<SessionId, Part> = BTreeMap::new();

    while let Some(row) = rows.next()? {
        let data: Vec<u8> = row.get(0)?;
        let Some(request) = Generation::decode(data.as_slice())
            .ok()
            .and_then(|generation| generation.request)
        else {
            continue;
        };
        let Some(usage) = &request.usage else {
            continue;
        };
        let tokens = Tokens {
            input: usage.input,
            output: usage.output,
            cache_read: usage.cache_read,
            cache_write: usage.cache_write,
            cache_write_long: 0,
        };

        if tokens.total() == 0 {
            continue;
        }

        let at = match request
            .label("last_step_index")
            .and_then(|step| step.parse::<i64>().ok())
        {
            Some(step) => steps
                .query_row([step], |row| row.get::<_, Vec<u8>>(0))
                .optional()?
                .and_then(|data| Step::decode(data.as_slice()).ok()?.created)
                .map_or(started, |time| time.seconds),
            None => started,
        };
        let Ok(at) = u64::try_from(at) else {
            continue;
        };

        if at < since {
            continue;
        }

        let session = request
            .label("root_cascade_id")
            .filter(|root| !root.is_empty())
            .unwrap_or(&own);
        let model = if request.model.is_empty() {
            UNKNOWN_MODEL
        } else {
            request.model.as_str()
        };
        let part = parts
            .entry(Into::<SessionId>::into(session.to_owned()))
            .or_default();
        part.cwd.clone_from(&cwd);
        part.buckets
            .entry(BucketKey {
                hour: at / 3600 * 3600,
                model: ModelId::new(model),
                billing,
            })
            .or_default()
            .add(model, &tokens, None);
    }

    Ok(parts)
}

// Antigravity's records are protobuf without a published schema; only the
// fields read here are named, by their numbers as the CLI writes them.

/// A `gen_metadata` row: one model request.
#[derive(Clone, PartialEq, Message)]
struct Generation {
    #[prost(message, optional, tag = "1")]
    request: Option<Request>,
}

#[derive(Clone, PartialEq, Message)]
struct Request {
    #[prost(message, optional, tag = "4")]
    usage: Option<Usage>,
    /// Such as `gemini-4-carbon`.
    #[prost(string, tag = "19")]
    model: String,
    /// Such as `root_cascade_id` and `last_step_index`.
    #[prost(message, repeated, tag = "20")]
    labels: Vec<Label>,
}

impl Request {
    fn label(&self, key: &str) -> Option<&String> {
        self.labels
            .iter()
            .find(|label| label.key == key)
            .map(|label| &label.value)
    }
}

/// `input` is read fresh, `output` counts thinking.
#[derive(Clone, PartialEq, Message)]
struct Usage {
    #[prost(uint64, tag = "2")]
    input: u64,
    #[prost(uint64, tag = "3")]
    output: u64,
    #[prost(uint64, tag = "4")]
    cache_write: u64,
    #[prost(uint64, tag = "5")]
    cache_read: u64,
}

#[derive(Clone, PartialEq, Message)]
struct Label {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

/// A `steps` row's `metadata`.
#[derive(Clone, PartialEq, Message)]
struct Step {
    #[prost(message, optional, tag = "1")]
    created: Option<Time>,
}

#[cfg(test)]
#[path = "../../tests/logs_spend_agy.rs"]
mod tests;
