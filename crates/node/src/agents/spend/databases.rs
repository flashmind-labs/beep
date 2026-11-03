//! What OpenCode and flsh sessions spent, from their SQLite databases,
//! read whole again whenever a database changes.
//!
//! OpenCode keeps each assistant message in `message.data` with `modelID`,
//! `tokens` (`input` read fresh, `output`, `reasoning`, `cache.read`,
//! `cache.write`) and `cost` (zero on a subscription), and `time_created`
//! in Unix milliseconds. flsh keeps only each session's totals, in
//! `session_meta.total_usage` (`input_tokens` counting cached input,
//! `cached_input_tokens`, `output_tokens`, `total_cost` as a decimal
//! string): they count in the hour of its last entry.

use super::ledger::{Bucket, BucketKey, Part, UNKNOWN_MODEL};
use crate::agents::prices::Tokens;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, time::Duration};

/// How long a locked database is waited on.
const BUSY: Duration = Duration::from_millis(200);
/// The most rows one database gives in a pass.
const MAX_ROWS: i64 = 50_000;

const OPENCODE: &str = "SELECT m.session_id, s.directory, (m.time_created / 3600000) * 3600,
        COALESCE(json_extract(m.data, '$.modelID'), ''),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.input'), 0)) AS INTEGER),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.output'), 0)
            + COALESCE(json_extract(m.data, '$.tokens.reasoning'), 0)) AS INTEGER),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.cache.read'), 0)) AS INTEGER),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.cache.write'), 0)) AS INTEGER),
        SUM(COALESCE(json_extract(m.data, '$.cost'), 0))
     FROM message m JOIN session s ON s.id = m.session_id
     WHERE m.time_created >= ?1 AND json_extract(m.data, '$.role') = 'assistant'
     GROUP BY 1, 2, 3, 4
     LIMIT ?2";

const FLSH: &str = "SELECT m.chat_key, m.working_dir, COALESCE(m.model, ''), m.total_usage,
        (SELECT MAX(s.created_at) FROM sessions s WHERE s.chat_key = m.chat_key) AS last
     FROM session_meta m
     WHERE m.total_usage IS NOT NULL AND m.working_dir IS NOT NULL AND m.working_dir != ''
       AND last >= ?1
     LIMIT ?2";

#[derive(Deserialize, Default)]
#[serde(default)]
struct FlshTotals {
    input_tokens: u64,
    cached_input_tokens: Option<u64>,
    output_tokens: u64,
    total_cost: String,
    tokens_unknown: bool,
    cost_unknown: bool,
}

fn open(database: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY)?;
    Ok(conn)
}

fn count(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// What each OpenCode session in `database` spent from `since` (Unix
/// seconds) on, by session.
pub(super) fn opencode(database: &Path, since: u64) -> rusqlite::Result<BTreeMap<String, Part>> {
    let conn = open(database)?;
    let mut statement = conn.prepare(OPENCODE)?;
    let since_ms = i64::try_from(since.saturating_mul(1000)).unwrap_or(i64::MAX);
    let mut rows = statement.query([since_ms, MAX_ROWS])?;
    let mut parts: BTreeMap<String, Part> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let session: String = row.get(0)?;
        let cwd: String = row.get(1)?;
        let hour = count(row.get(2)?);
        let model: String = row.get(3)?;
        let tokens = Tokens {
            input: count(row.get(4)?),
            output: count(row.get(5)?),
            cache_read: count(row.get(6)?),
            cache_write: count(row.get(7)?),
            cache_write_long: 0,
        };
        let cost: f64 = row.get(8)?;
        if tokens.total() == 0 && cost <= 0.0 {
            continue;
        }
        let model = if model.is_empty() {
            UNKNOWN_MODEL.to_owned()
        } else {
            model
        };
        let part = parts.entry(session).or_default();
        part.cwd = cwd;
        part.buckets
            .entry(BucketKey {
                hour,
                model: model.clone(),
            })
            .or_default()
            .add(&model, &tokens, Some(cost));
    }
    Ok(parts)
}

/// What each flsh session in `database` active from `since` (Unix
/// seconds) on spent, by session. A database from before flsh kept usage
/// has no `total_usage` and fails.
pub(super) fn flsh(database: &Path, since: u64) -> rusqlite::Result<BTreeMap<String, Part>> {
    let conn = open(database)?;
    let mut statement = conn.prepare(FLSH)?;
    let since = i64::try_from(since).unwrap_or(i64::MAX);
    let mut rows = statement.query([since, MAX_ROWS])?;
    let mut parts: BTreeMap<String, Part> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let session: String = row.get(0)?;
        let cwd: String = row.get(1)?;
        let model: String = row.get(2)?;
        let totals: String = row.get(3)?;
        let last: Option<i64> = row.get(4)?;
        let Ok(totals) = serde_json::from_str::<FlshTotals>(&totals) else {
            continue;
        };
        if totals.tokens_unknown {
            continue;
        }
        let cached = totals
            .cached_input_tokens
            .unwrap_or(0)
            .min(totals.input_tokens);
        let tokens = Tokens {
            input: totals.input_tokens - cached,
            output: totals.output_tokens,
            cache_read: cached,
            cache_write: 0,
            cache_write_long: 0,
        };
        let cost = totals
            .total_cost
            .parse::<f64>()
            .ok()
            .filter(|cost| cost.is_finite() && *cost > 0.0);
        if tokens.total() == 0 && cost.is_none() {
            continue;
        }
        let last = count(last.unwrap_or(0));
        let model = match model.as_str() {
            "" => UNKNOWN_MODEL.to_owned(),
            _ => model,
        };
        let mut bucket = Bucket::default();
        match cost {
            Some(cost) => bucket.add(&model, &tokens, Some(cost)),
            // flsh found no price; neither would the table, mostly.
            None if totals.cost_unknown => {
                bucket.tokens = tokens;
                bucket.unpriced = tokens.total();
            }
            None => bucket.add(&model, &tokens, None),
        }
        let part = parts.entry(session).or_default();
        part.cwd = cwd;
        part.buckets.insert(
            BucketKey {
                hour: last - last % 3600,
                model,
            },
            bucket,
        );
    }
    Ok(parts)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// An OpenCode database with two sessions' messages.
    pub(crate) fn opencode_database(path: &Path) {
        let conn = Connection::open(path).expect("a database");
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT NOT NULL);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                 time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
             INSERT INTO session VALUES ('ses_a', '/srv/project'), ('ses_b', '/srv/other');
             INSERT INTO message VALUES
               ('m1', 'ses_a', 1793614500000, 0, '{\"role\":\"user\"}'),
               ('m2', 'ses_a', 1793614600000, 0, '{\"role\":\"assistant\",\"modelID\":\"gpt-6.1-sol\",\"cost\":0,\"tokens\":{\"input\":1000000,\"output\":100000,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}}}'),
               ('m3', 'ses_b', 1793614700000, 0, '{\"role\":\"assistant\",\"modelID\":\"example-model\",\"cost\":0.25,\"tokens\":{\"input\":10,\"output\":10,\"cache\":{\"read\":0,\"write\":0}}}'),
               ('m4', 'ses_b', 1000, 0, '{\"role\":\"assistant\",\"modelID\":\"example-model\",\"cost\":9,\"tokens\":{\"input\":10}}');",
        )
        .expect("the schema");
    }

    #[test]
    fn opencode_sessions_are_priced_by_their_cost_or_the_table() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let path = dir.path().join("opencode.db");
        opencode_database(&path);
        let parts = opencode(&path, 1_793_600_000).expect("the sessions");
        let a = &parts["ses_a"];
        assert_eq!(a.cwd, "/srv/project");
        let bucket = a.buckets.values().next().expect("a bucket");
        // 1M input at $2 and 100K output at $10.
        assert!((bucket.cost_usd - 3.0).abs() < 1e-9);
        let b = parts["ses_b"].buckets.values().next().expect("a bucket");
        assert!(
            (b.cost_usd - 0.25).abs() < 1e-9,
            "the old message is left out"
        );
    }

    #[test]
    fn flsh_sessions_count_their_totals_at_their_last_entry() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let path = dir.path().join("sessions.db");
        let conn = Connection::open(&path).expect("a database");
        conn.execute_batch(
            "CREATE TABLE sessions (id INTEGER PRIMARY KEY, chat_key TEXT NOT NULL,
                 entry_kind TEXT NOT NULL, content TEXT NOT NULL DEFAULT '',
                 turn_index INTEGER NOT NULL, created_at INTEGER NOT NULL);
             CREATE TABLE session_meta (chat_key TEXT PRIMARY KEY, title TEXT, model TEXT,
                 working_dir TEXT, created_at INTEGER NOT NULL, total_cost TEXT,
                 last_usage TEXT, total_usage TEXT);
             INSERT INTO session_meta VALUES ('c1', 'Fix', 'openrouter:example/model', '/srv/project',
                 1793600000, '0.42', NULL,
                 '{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":50,\"total_cost\":\"0.42\",\"tokens_unknown\":false,\"cost_unknown\":false}');
             INSERT INTO sessions (chat_key, entry_kind, turn_index, created_at)
                 VALUES ('c1', '{\"type\":\"user\"}', 0, 1793600000), ('c1', 'x', 1, 1793614500);",
        )
        .expect("the schema");
        let parts = flsh(&path, 1_793_000_000).expect("the sessions");
        let (key, bucket) = parts["c1"].buckets.iter().next().expect("a bucket");
        assert_eq!(key.hour, 1_793_613_600);
        assert_eq!(bucket.tokens.cache_read, 400);
        assert_eq!(bucket.tokens.input, 600);
        assert!((bucket.cost_usd - 0.42).abs() < 1e-9);
        assert!(flsh(&path, 1_793_700_000).expect("none").is_empty());
    }
}
