//! What OpenCode and flsh sessions spent (`opencode`, `flsh`, `flsh_file`),
//! from their SQLite databases and flsh's session files, read whole again
//! whenever one changes.

use super::{
    billing::{Accounts, flsh_provider},
    ledger::{Bucket, BucketKey, Counted, Part, UNKNOWN_MODEL},
};
use crate::agents::found::unix_seconds;
use crate::agents::found::{Agent, open_database};
use crate::agents::prices::Tokens;
use crate::agents::window::{head_lines, tail_lines};
use beep_proto::{
    ModelId, SessionId,
    node::usage::{Billing, HistoricalSpend},
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

/// The most rows one database gives in a pass.
const MAX_ROWS: i64 = 50_000;

/// Each assistant message's `message.data`: `modelID`, `tokens` (`input`
/// read fresh), `cost` (zero on a subscription) and `providerID`, which
/// tells how it was paid for.
const OPENCODE: &str = "SELECT m.session_id, s.directory, (m.time_created / 3600000) * 3600,
        COALESCE(json_extract(m.data, '$.modelID'), ''),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.input'), 0)) AS INTEGER),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.output'), 0)
            + COALESCE(json_extract(m.data, '$.tokens.reasoning'), 0)) AS INTEGER),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.cache.read'), 0)) AS INTEGER),
        CAST(SUM(COALESCE(json_extract(m.data, '$.tokens.cache.write'), 0)) AS INTEGER),
        SUM(COALESCE(json_extract(m.data, '$.cost'), 0)),
        COALESCE(json_extract(m.data, '$.providerID'), '')
     FROM message m JOIN session s ON s.id = m.session_id
     WHERE m.time_created >= ?1 AND json_extract(m.data, '$.role') = 'assistant'
     GROUP BY 1, 2, 3, 4, 10
     LIMIT ?2";

/// Each session's totals, `session_meta.total_usage` (`input_tokens`
/// counting cached input, `total_cost` a decimal string), with the
/// session's date range: the database keeps no dated usage.
const FLSH: &str = "SELECT m.chat_key, m.working_dir, COALESCE(m.model, ''), m.total_usage,
        (SELECT MAX(s.created_at) FROM sessions s WHERE s.chat_key = m.chat_key) AS last,
        (SELECT MIN(s.created_at) FROM sessions s WHERE s.chat_key = m.chat_key)
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
    cost_unknown: bool,
}

fn count(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// What each OpenCode session in `database` spent from `since` (Unix
/// seconds) on, by session.
pub(super) fn opencode(
    database: &Path,
    since: u64,
    accounts: &mut Accounts,
) -> rusqlite::Result<BTreeMap<SessionId, Part>> {
    let conn = open_database(database)?;
    let mut statement = conn.prepare(OPENCODE)?;
    let since_ms = i64::try_from(since.saturating_mul(1000)).unwrap_or(i64::MAX);
    let mut rows = statement.query([since_ms, MAX_ROWS])?;
    let mut parts: BTreeMap<SessionId, Part> = BTreeMap::new();

    while let Some(row) = rows.next()? {
        let session = Into::<SessionId>::into(row.get::<_, String>(0)?);
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
        let provider: String = row.get(9)?;

        if tokens.total() == 0 && cost <= 0.0 {
            continue;
        }

        let billing = accounts.resolve(
            Agent::OpenCode,
            Billing::Unknown,
            Some(provider.as_str()).filter(|provider| !provider.is_empty()),
            Some(cwd.as_str()),
        );
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
                model: ModelId::new(model.as_str()),
                billing,
            })
            .or_default()
            .add(&model, &tokens, Some(cost));
    }

    Ok(parts)
}

/// What each flsh session in `database` active from `since` (Unix
/// seconds) on spent, by session. A database from before flsh kept usage
/// has no `total_usage` and fails.
pub(super) fn flsh(
    database: &Path,
    since: u64,
    accounts: &mut Accounts,
) -> rusqlite::Result<BTreeMap<SessionId, Part>> {
    let conn = open_database(database)?;
    let mut statement = conn.prepare(FLSH)?;
    let since = i64::try_from(since).unwrap_or(i64::MAX);
    let mut rows = statement.query([since, MAX_ROWS])?;
    let mut parts: BTreeMap<SessionId, Part> = BTreeMap::new();

    while let Some(row) = rows.next()? {
        let totals: String = row.get(3)?;
        let Ok(totals) = serde_json::from_str::<FlshTotals>(&totals) else {
            continue;
        };
        let last: Option<i64> = row.get(4)?;
        let session = FlshSession {
            id: Into::<SessionId>::into(row.get::<_, String>(0)?),
            cwd: row.get(1)?,
            model: row.get(2)?,
            totals,
            turns: Vec::new(),
            first: row.get::<_, Option<i64>>(5)?.map(count),
            last: last.map(count),
        };
        add_flsh(&mut parts, session, None, accounts);
    }

    Ok(parts)
}

/// What a flsh session file spent beyond `counted`, which was counted
/// where it ran before it moved here: each turn of its header's
/// `usage_log` in its own hour, and what its `total_usage` has beyond them
/// as a historical total with transcript bounds.
pub(super) fn flsh_file(
    path: &Path,
    counted: Option<&Counted>,
    accounts: &mut Accounts,
) -> std::io::Result<BTreeMap<SessionId, Part>> {
    let mut parts = BTreeMap::new();

    if let Some(session) = flsh_header(path)? {
        let id = session.id.clone();
        let cwd = session.cwd.clone();
        let routed = flsh_provider(&session.model) == Some("openrouter");
        add_flsh(&mut parts, session, counted, accounts);
        if let Some(legacy) = parts.get_mut(&id).and_then(|p| p.historical.as_mut())
            && legacy.last.is_none()
        {
            legacy.last = tail_lines(path, MAX_HEADER)?
                .filter_map(message_time)
                .last();
        }
        let billed = path.with_extension("openrouter.json");
        match std::fs::File::open(billed) {
            Ok(file) => {
                let hours: BTreeMap<u64, BTreeMap<String, BilledUsage>> =
                    serde_json::from_reader(std::io::BufReader::new(file))?;
                let part = parts.entry(id).or_default();
                part.cwd = cwd;
                replace_billed_hours(part, hours, routed)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }

    Ok(parts)
}

/// Finalized OpenRouter hours on this machine. Each model's input includes cache reads.
#[derive(Deserialize)]
struct BilledUsage {
    input: u64,
    cached: u64,
    output: u64,
    usd: f64,
}

fn replace_billed_hours(
    part: &mut Part,
    hours: BTreeMap<u64, BTreeMap<String, BilledUsage>>,
    routed: bool,
) -> std::io::Result<()> {
    for (hour, models) in hours {
        if !hour.is_multiple_of(3600) || models.is_empty() {
            return Err(std::io::Error::other("invalid OpenRouter billing hour"));
        }
        let mut replaced_tokens = 0_u64;
        let mut replaced_cost = 0.0;
        part.buckets.retain(|key, bucket| {
            let replace =
                key.hour == hour && flsh_provider(key.model.as_ref()) == Some("openrouter");
            if replace {
                replaced_tokens = replaced_tokens.saturating_add(bucket.tokens.total());
                replaced_cost += bucket.cost_usd;
            }
            !replace
        });
        if routed
            && let Some(legacy) = &mut part.historical
            && legacy.first.is_none_or(|first| first / 3600 <= hour / 3600)
            && legacy.last.is_none_or(|last| last / 3600 >= hour / 3600)
        {
            let billed = models.values().fold(0_u64, |sum, u| {
                sum.saturating_add(u.input).saturating_add(u.output)
            });
            let extra = billed.saturating_sub(replaced_tokens);
            if extra > 0 {
                legacy.tokens = legacy.tokens.saturating_sub(extra);
                let cost: f64 = models.values().map(|u| u.usd).sum();
                legacy.cost_usd = (legacy.cost_usd - (cost - replaced_cost).max(0.0)).max(0.0);
                if legacy.tokens == 0 {
                    part.historical = None;
                }
            }
        }
        for (model, usage) in models {
            if model.is_empty()
                || usage.cached > usage.input
                || !usage.usd.is_finite()
                || usage.usd < 0.0
            {
                return Err(std::io::Error::other("invalid OpenRouter billing totals"));
            }
            part.buckets.insert(
                BucketKey {
                    hour,
                    model: ModelId::new(format!("openrouter:{model}").as_str()),
                    billing: Billing::Metered,
                },
                Bucket {
                    tokens: Tokens {
                        input: usage.input - usage.cached,
                        output: usage.output,
                        cache_read: usage.cached,
                        ..Tokens::default()
                    },
                    cost_usd: usage.usd,
                    unpriced: 0,
                },
            );
        }
    }
    Ok(())
}

/// The totals in the header of the flsh session file at `path`, when it
/// has them.
pub(super) fn flsh_counted(path: &Path) -> std::io::Result<Option<Counted>> {
    Ok(flsh_header(path)?.map(|session| Counted::from(&session.totals)))
}

fn flsh_header(path: &Path) -> std::io::Result<Option<FlshSession>> {
    let mut lines = head_lines(path, MAX_HEADER)?;
    let Some(Ok(header)) = lines.next().map(|l| serde_json::from_str::<FlshHeader>(&l)) else {
        return Ok(None);
    };
    let first = lines.find_map(message_time);
    let Some(cwd) = header.metadata.working_dir else {
        return Ok(None);
    };
    Ok(Some(FlshSession {
        id: header.session_id,
        cwd,
        model: header.metadata.model.unwrap_or_default(),
        totals: header.metadata.total_usage.unwrap_or_default(),
        turns: header.metadata.usage_log,
        first,
        last: None,
    }))
}

/// System prompts can be rewritten on resume.
fn message_time(line: String) -> Option<u64> {
    let message: FlshMessage = serde_json::from_str(&line).ok()?;
    match message.kind.as_str() {
        "user" | "assistant" | "tool" => unix_seconds(&message.timestamp),
        _ => None,
    }
}

/// The longest flsh header read.
const MAX_HEADER: u64 = 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlshHeader {
    session_id: SessionId,
    metadata: FlshMetadata,
}

#[derive(Deserialize)]
struct FlshMessage {
    #[serde(rename = "type")]
    kind: String,
    timestamp: String,
}

#[derive(Deserialize)]
struct FlshMetadata {
    model: Option<String>,
    working_dir: Option<String>,
    total_usage: Option<FlshTotals>,
    #[serde(default)]
    usage_log: Vec<FlshTurn>,
}

/// What one flsh turn spent, from its session header's `usage_log`.
#[derive(Deserialize)]
struct FlshTurn {
    /// Unix seconds.
    at: u64,
    model: Option<String>,
    /// Counting cached input.
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    /// USD as a decimal string.
    cost: String,
    cost_known: Option<bool>,
}

impl From<&FlshTurn> for Counted {
    fn from(turn: &FlshTurn) -> Self {
        Self {
            input: turn.input_tokens,
            cached: turn.cached_input_tokens,
            output: turn.output_tokens,
            cost: turn.cost.parse().unwrap_or(0.0),
        }
    }
}

impl From<&FlshTotals> for Counted {
    fn from(totals: &FlshTotals) -> Self {
        Self {
            input: totals.input_tokens,
            cached: totals.cached_input_tokens.unwrap_or(0),
            output: totals.output_tokens,
            cost: totals.total_cost.parse().unwrap_or(0.0),
        }
    }
}

/// One flsh session's totals and the turns it logged.
struct FlshSession {
    id: SessionId,
    cwd: String,
    model: String,
    totals: FlshTotals,
    turns: Vec<FlshTurn>,
    /// Transcript bounds, when recorded.
    first: Option<u64>,
    last: Option<u64>,
}

/// Adds what `session` spent beyond `counted` to `parts`: each turn in its
/// hour, and what the totals have beyond the turns, spent before flsh
/// logged turns, as a historical balance. `counted` is taken from the oldest spend first.
fn add_flsh(
    parts: &mut BTreeMap<SessionId, Part>,
    session: FlshSession,
    counted: Option<&Counted>,
    accounts: &mut Accounts,
) {
    let FlshSession {
        id,
        cwd,
        model,
        totals,
        turns,
        first,
        last,
    } = session;

    let mut logged = Counted::default();

    for turn in &turns {
        logged.add(Counted::from(turn));
    }

    let (earlier, _) = Counted::from(&totals).beyond(logged);
    let (legacy, mut before) = earlier.beyond(counted.copied().unwrap_or_default());
    let part = parts.entry(id.clone()).or_default();
    part.cwd.clone_from(&cwd);
    if legacy.input != 0 || legacy.output != 0 || legacy.cost > 0.0 {
        part.historical = Some(HistoricalSpend {
            first,
            last: turns.iter().map(|turn| turn.at).min().or(last),
            tokens: legacy.input.saturating_add(legacy.output),
            cost_usd: legacy.cost,
            billing: accounts.resolve(
                Agent::Flsh,
                Billing::Unknown,
                flsh_provider(&model),
                Some(&cwd),
            ),
        });
    }

    for turn in &turns {
        let at = turn.at;
        let model = turn.model.clone().unwrap_or_else(|| model.clone());
        let cost_known = turn.cost_known;
        let (spend, left) = Counted::from(turn).beyond(before);
        before = left;
        let cached = spend.cached.min(spend.input);
        let tokens = Tokens {
            input: spend.input - cached,
            output: spend.output,
            cache_read: cached,
            cache_write: 0,
            cache_write_long: 0,
        };
        let cost = Some(spend.cost).filter(|cost| cost.is_finite() && *cost > 0.0);

        if tokens.total() == 0 && cost.is_none() {
            continue;
        }

        let billing = accounts.resolve(
            Agent::Flsh,
            Billing::Unknown,
            flsh_provider(&model),
            Some(cwd.as_str()),
        );
        let model = match model.as_str() {
            "" => UNKNOWN_MODEL.to_owned(),
            _ => model,
        };
        let bucket = part
            .buckets
            .entry(BucketKey {
                hour: at - at % 3600,
                model: ModelId::new(model.as_str()),
                billing,
            })
            .or_default();

        match cost {
            Some(cost) => bucket.add(&model, &tokens, Some(cost)),
            // flsh found no price; neither would the table, mostly.
            None if cost_known == Some(true) => bucket.tokens.add(&tokens),
            None if cost_known == Some(false) || totals.cost_unknown => {
                bucket.tokens.add(&tokens);
                bucket.unpriced = bucket.unpriced.saturating_add(tokens.total());
            }
            None => bucket.add(&model, &tokens, None),
        }
    }

    if part.buckets.is_empty() && part.historical.is_none() {
        parts.remove(&id);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;

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
               ('m2', 'ses_a', 1793614600000, 0, '{\"role\":\"assistant\",\"providerID\":\"github-copilot\",\"modelID\":\"gpt-6.1-sol\",\"cost\":0,\"tokens\":{\"input\":1000000,\"output\":100000,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}}}'),
               ('m3', 'ses_b', 1793614700000, 0, '{\"role\":\"assistant\",\"modelID\":\"example-model\",\"cost\":0.25,\"tokens\":{\"input\":10,\"output\":10,\"cache\":{\"read\":0,\"write\":0}}}'),
               ('m4', 'ses_b', 1000, 0, '{\"role\":\"assistant\",\"modelID\":\"example-model\",\"cost\":9,\"tokens\":{\"input\":10}}');",
        )
        .expect("the schema");
    }

    #[test]
    fn a_flsh_session_file_counts_its_totals_beyond_what_was_counted_elsewhere() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let path = dir.path().join("a.jsonl");
        let usage = |input: u64, cached: u64, cost: &str| {
            format!(
                "{{\"type\":\"system\",\"subtype\":\"session_metadata\",\"version\":2,\"sessionId\":\"c1\",\"metadata\":{{\"model\":\"openrouter:example/model\",\"working_dir\":\"/srv/project\",\"total_usage\":{{\"input_tokens\":{input},\"cached_input_tokens\":{cached},\"output_tokens\":50,\"total_cost\":\"{cost}\",\"tokens_unknown\":false,\"cost_unknown\":false}}}}}}\n{{\"type\":\"user\",\"timestamp\":\"2026-10-31T08:02:00Z\"}}\n"
            )
        };
        fs::write(&path, usage(1000, 400, "0.42")).expect("a session file");
        let accounts = &mut Accounts::default();
        let parts = flsh_file(&path, None, accounts).expect("the session");
        let bucket = parts["c1"].historical.as_ref().expect("historical total");
        assert_eq!(bucket.tokens, 1050);
        assert!((bucket.cost_usd - 0.42).abs() < 1e-9);
        assert_eq!(parts["c1"].cwd, "/srv/project");

        // It moved here at these totals, then spent more.
        let counted = flsh_counted(&path).expect("read").expect("totals");
        fs::write(&path, usage(1500, 500, "0.52")).expect("more");
        let parts = flsh_file(&path, Some(&counted), accounts).expect("the session");
        let bucket = parts["c1"].historical.as_ref().expect("historical total");
        assert_eq!(bucket.tokens, 500);
        assert!((bucket.cost_usd - 0.10).abs() < 1e-9);
    }

    #[test]
    fn a_flsh_session_file_counts_each_logged_turn_in_its_hour() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let path = dir.path().join("a.jsonl");
        let turn = |at: u64, input: u64, cost: &str| {
            format!(
                "{{\"at\":{at},\"model\":\"openrouter:example/model\",\"input_tokens\":{input},\"cached_input_tokens\":0,\"output_tokens\":10,\"cost\":\"{cost}\"}}"
            )
        };
        // 0.10 spent before turns were logged, from its start two days ago, then
        // a turn a day ago and one now.
        let log = [
            turn(1_793_520_000, 300, "0.20"),
            turn(1_793_606_400, 500, "0.30"),
        ]
        .join(",");
        fs::write(
            &path,
            format!(
                "{{\"type\":\"system\",\"subtype\":\"session_metadata\",\"version\":2,\"sessionId\":\"c1\",\"metadata\":{{\"model\":\"openrouter:example/model\",\"working_dir\":\"/srv/project\",\"total_usage\":{{\"input_tokens\":1000,\"cached_input_tokens\":0,\"output_tokens\":30,\"total_cost\":\"0.60\",\"tokens_unknown\":false,\"cost_unknown\":false}},\"usage_log\":[{log}]}}}}\n{{\"type\":\"user\",\"timestamp\":\"2026-10-31T08:02:00Z\"}}\n"
            ),
        )
        .expect("a session file");
        let accounts = &mut Accounts::default();
        let costs = |counted: Option<&Counted>| -> Vec<(u64, f64)> {
            let parts = flsh_file(&path, counted, &mut Accounts::default()).expect("the session");
            parts["c1"]
                .buckets
                .iter()
                .map(|(key, bucket)| (key.hour, (bucket.cost_usd * 100.0).round() / 100.0))
                .collect()
        };
        assert_eq!(
            costs(None),
            [(1_793_520_000, 0.20), (1_793_606_400, 0.30)],
            "only dated turns count toward daily spend"
        );

        // It moved here after the first turn: only the second counts.
        let moved = Counted {
            input: 500,
            cached: 0,
            output: 20,
            cost: 0.30,
        };
        assert_eq!(costs(Some(&moved)), [(1_793_606_400, 0.30)]);
        let parts = flsh_file(&path, Some(&moved), accounts).expect("the session");
        let bucket = parts["c1"].buckets.values().next().expect("a bucket");
        assert_eq!((bucket.tokens.input, bucket.tokens.output), (500, 10));
    }

    #[test]
    fn flsh_database_totals_keep_their_session_range() {
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
        let accounts = &mut Accounts::default();
        let parts = flsh(&path, 1_793_000_000, accounts).expect("the sessions");
        let usage = parts["c1"].historical.as_ref().unwrap();
        assert_eq!(
            (usage.first, usage.last),
            (Some(1793600000), Some(1793614500))
        );
        assert_eq!(usage.billing, Billing::Metered);
        assert_eq!(usage.tokens, 1050);
        assert!((usage.cost_usd - 0.42).abs() < 1e-9);
        assert!(
            flsh(&path, 1_793_700_000, accounts)
                .expect("none")
                .is_empty()
        );
    }
}
