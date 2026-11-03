//! What the agents' logs said so far: per log file, how far it was read
//! and what each session in it spent by hour and model, and which sessions
//! changed since the server last heard of them. Kept in one JSON file so a
//! restart reads only what was written since.

use super::{Line, billing::Accounts};
use crate::{
    agents::prices::{self, Tokens},
    found::Agent,
};
use beep_proto::{
    ModelId,
    node::usage::{Billing, HourSpend, MAX_SESSION_HOURS, SessionSpend},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// How many message keys of a Claude Code log are kept to drop the copies
/// it writes of one message, one per content block, a few lines apart.
const RECENT_KEYS: usize = 64;
/// The model of a request whose log does not name one.
pub(super) const UNKNOWN_MODEL: &str = "unknown";
/// The ledger's shape: one saved in another is dropped and the logs read
/// again. 1 marks each bucket's billing.
const VERSION: u32 = 1;

/// A session, by its agent and the agent's own id for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct SessionKey {
    pub(super) agent: Agent,
    pub(super) session: String,
}

/// One hour of one model, paid for one way.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct BucketKey {
    /// Unix seconds at the start of the hour.
    pub(super) hour: u64,
    pub(super) model: ModelId,
    #[serde(default)]
    pub(super) billing: Billing,
}

/// What was spent in one bucket.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Bucket {
    pub(super) tokens: Tokens,
    pub(super) cost_usd: f64,
    /// Tokens of requests no price covered.
    pub(super) unpriced: u64,
}

impl Bucket {
    /// Adds a request: at `cost_usd` when its log said, else at the model's
    /// list price, else unpriced.
    pub(super) fn add(&mut self, model: &str, tokens: &Tokens, cost_usd: Option<f64>) {
        self.tokens.add(tokens);
        let cost = cost_usd
            .filter(|cost| cost.is_finite() && *cost > 0.0)
            .or_else(|| prices::price(model).map(|price| price.cost(tokens)));
        match cost {
            Some(cost) => self.cost_usd += cost,
            None => self.unpriced = self.unpriced.saturating_add(tokens.total()),
        }
    }

    fn merge(&mut self, other: &Bucket) {
        self.tokens.add(&other.tokens);
        self.cost_usd += other.cost_usd;
        self.unpriced = self.unpriced.saturating_add(other.unpriced);
    }
}

/// What one session spent in one log.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Part {
    pub(super) cwd: String,
    /// A list in JSON, whose map keys are strings only.
    #[serde(with = "entries")]
    pub(super) buckets: BTreeMap<BucketKey, Bucket>,
}

/// A file's size and modification time, which tell it changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Stamp {
    pub(super) len: u64,
    pub(super) modified_ns: u128,
}

impl Stamp {
    pub(super) fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        let modified_ns = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        Ok(Self {
            len: meta.len(),
            modified_ns,
        })
    }
}

/// How far one JSONL log was read and what it said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct LogFile {
    pub(super) agent: Option<Agent>,
    pub(super) stamp: Stamp,
    /// Read up to here: the end of the last whole line.
    pub(super) read: u64,
    /// The session and folder its header named, for logs whose lines do not.
    pub(super) session: Option<String>,
    pub(super) cwd: Option<String>,
    /// The model the next requests run on, for Codex.
    pub(super) model: Option<String>,
    /// The provider its header named, for Codex.
    #[serde(default)]
    pub(super) provider: Option<String>,
    /// Codex's running total, to drop a count it repeats.
    pub(super) total: Option<u64>,
    /// The latest message keys, to drop Claude Code's copies.
    pub(super) recent: VecDeque<u64>,
    pub(super) parts: BTreeMap<String, Part>,
}

impl LogFile {
    pub(super) fn new(agent: Agent) -> Self {
        Self {
            agent: Some(agent),
            ..Self::default()
        }
    }

    /// Takes one line in, telling how its request was paid for by
    /// `accounts`; the session it added to, if any.
    pub(super) fn take(&mut self, line: Line, accounts: &mut Accounts) -> Option<String> {
        match line {
            Line::Session { id, cwd, provider } => {
                self.session = Some(id);
                if !cwd.is_empty() {
                    self.cwd = Some(cwd);
                }
                if provider.is_some() {
                    self.provider = provider;
                }
                None
            }
            Line::Model(model) => {
                self.model = Some(model);
                None
            }
            Line::Request(request) => {
                if let Some(key) = request.key {
                    if self.recent.contains(&key) {
                        return None;
                    }
                    self.recent.push_back(key);
                    while self.recent.len() > RECENT_KEYS {
                        self.recent.pop_front();
                    }
                }
                if let Some(total) = request.total {
                    if self.total == Some(total) {
                        return None;
                    }
                    self.total = Some(total);
                }
                let session = request.session.or_else(|| self.session.clone())?;
                let model = request
                    .model
                    .or_else(|| self.model.clone())
                    .unwrap_or_else(|| UNKNOWN_MODEL.to_owned());
                let cwd = request.cwd.or_else(|| self.cwd.clone());
                let provider = request.provider.or_else(|| self.provider.clone());
                let billing = self.agent.map_or(Billing::Unknown, |agent| {
                    accounts.resolve(agent, request.billing, provider.as_deref(), cwd.as_deref())
                });
                let part = self.parts.entry(session.clone()).or_default();
                if part.cwd.is_empty() {
                    part.cwd = cwd.unwrap_or_default();
                }
                let hour = request.at - request.at % 3600;
                part.buckets
                    .entry(BucketKey {
                        hour,
                        model: ModelId::new(model.as_str()),
                        billing,
                    })
                    .or_default()
                    .add(&model, &request.tokens, request.cost_usd);
                Some(session)
            }
        }
    }
}

/// What one database said when it last changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Database {
    pub(super) agent: Option<Agent>,
    pub(super) stamp: Stamp,
    pub(super) parts: BTreeMap<String, Part>,
}

/// Everything read so far.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Ledger {
    /// `VERSION` when saved; 0 from before it was kept.
    #[serde(default)]
    pub(super) version: u32,
    pub(super) logs: BTreeMap<PathBuf, LogFile>,
    pub(super) databases: BTreeMap<PathBuf, Database>,
    /// Sessions the server has not heard of as they stand.
    pub(super) unsent: BTreeSet<SessionKey>,
    /// Whether it changed since it was saved.
    #[serde(skip)]
    pub(super) changed: bool,
}

impl Ledger {
    /// The ledger saved at `path`; an empty one when there is none or it
    /// cannot be read, which only means reading the logs again.
    pub(super) fn load(path: &Path) -> Self {
        let read = fs::read(path)
            .and_then(|bytes| serde_json::from_slice::<Ledger>(&bytes).map_err(io::Error::other));
        match read {
            Ok(ledger) if ledger.version == VERSION => ledger,
            Ok(_) => {
                log::info!("agent usage ledger from another version; reading the logs again");
                Self::default()
            }
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!(
                        "agent usage ledger not read; reading the logs again; path={} error={e}",
                        path.display()
                    );
                }
                Self::default()
            }
        }
    }

    /// Saves it at `path` when it changed, whole or not at all.
    pub(super) fn save(&mut self, path: &Path) -> io::Result<()> {
        if !self.changed {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        self.version = VERSION;
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        let partial = path.with_extension("json.partial");
        fs::write(&partial, bytes)?;
        fs::rename(&partial, path)?;
        self.changed = false;
        Ok(())
    }

    /// The session `key` as the server should keep it from `since` on:
    /// every log's and database's part of it together.
    pub(super) fn report(&self, key: &SessionKey, since: u64) -> Option<SessionSpend> {
        let parts = self
            .logs
            .values()
            .filter(|log| log.agent == Some(key.agent))
            .filter_map(|log| log.parts.get(&key.session))
            .chain(
                self.databases
                    .values()
                    .filter(|database| database.agent == Some(key.agent))
                    .filter_map(|database| database.parts.get(&key.session)),
            );
        let mut cwd = String::new();
        let mut buckets: BTreeMap<&BucketKey, Bucket> = BTreeMap::new();
        let mut any = false;
        for part in parts {
            any = true;
            if cwd.is_empty() {
                cwd.clone_from(&part.cwd);
            }
            for (bucket, spent) in &part.buckets {
                buckets.entry(bucket).or_default().merge(spent);
            }
        }
        if !any {
            return None;
        }
        // The newest hours when there are too many.
        let skip = buckets.len().saturating_sub(MAX_SESSION_HOURS);
        let hours = buckets
            .into_iter()
            .skip(skip)
            .map(|(bucket, spent)| HourSpend {
                hour: bucket.hour,
                model: bucket.model.clone(),
                input_tokens: spent.tokens.input,
                output_tokens: spent.tokens.output,
                cache_read_tokens: spent.tokens.cache_read,
                cache_write_tokens: spent
                    .tokens
                    .cache_write
                    .saturating_add(spent.tokens.cache_write_long),
                cost_usd: spent.cost_usd,
                unpriced_tokens: spent.unpriced,
                billing: bucket.billing,
            })
            .collect();
        Some(SessionSpend {
            agent: key.agent.slug(),
            session: key.session.clone(),
            cwd,
            since,
            hours,
        })
    }

    /// The latest hour `key` spent in, to send the newest sessions first.
    pub(super) fn last_hour(&self, key: &SessionKey) -> u64 {
        let parts = self
            .logs
            .values()
            .filter_map(|log| log.parts.get(&key.session))
            .chain(
                self.databases
                    .values()
                    .filter_map(|database| database.parts.get(&key.session)),
            );
        parts
            .filter_map(|part| part.buckets.keys().next_back())
            .map(|bucket| bucket.hour)
            .max()
            .unwrap_or(0)
    }
}

/// A map as a list of its entries, for keys JSON cannot take.
mod entries {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub(super) fn serialize<S, K, V>(map: &BTreeMap<K, V>, to: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Serialize,
        V: Serialize,
    {
        to.collect_seq(map.iter())
    }

    pub(super) fn deserialize<'de, D, K, V>(from: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
    {
        Vec::<(K, V)>::deserialize(from).map(|entries| entries.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::spend::Request;

    fn request(session: &str, at: u64, key: Option<u64>, input: u64) -> Line {
        Line::Request(Request {
            session: Some(session.into()),
            cwd: Some("/srv/project".into()),
            at,
            model: Some("claude-opus-5".into()),
            tokens: Tokens {
                input,
                ..Tokens::default()
            },
            cost_usd: None,
            key,
            total: None,
            provider: None,
            billing: Billing::Unknown,
        })
    }

    #[test]
    fn copies_of_a_message_count_once_and_hours_split() {
        let accounts = &mut Accounts::default();
        let mut log = LogFile::new(Agent::Claude);
        assert_eq!(
            log.take(request("s1", 7_200, Some(1), 1_000_000), accounts)
                .as_deref(),
            Some("s1")
        );
        assert_eq!(
            log.take(request("s1", 7_201, Some(1), 1_000_000), accounts),
            None
        );
        log.take(request("s1", 10_900, Some(2), 2_000_000), accounts);
        let part = &log.parts["s1"];
        assert_eq!(part.cwd, "/srv/project");
        let costs: Vec<(u64, f64)> = part
            .buckets
            .iter()
            .map(|(key, bucket)| (key.hour, bucket.cost_usd))
            .collect();
        assert_eq!(costs, [(7_200, 5.0), (10_800, 10.0)]);
    }

    #[test]
    fn a_repeated_running_total_counts_once() {
        let accounts = &mut Accounts::default();
        let mut log = LogFile::new(Agent::Codex);
        log.take(
            Line::Session {
                id: "t1".into(),
                cwd: "/srv/project".into(),
                provider: Some("openrouter".into()),
            },
            accounts,
        );
        log.take(Line::Model("gpt-9-unknown".into()), accounts);
        let count = |total| {
            Line::Request(Request {
                at: 100,
                tokens: Tokens {
                    input: 10,
                    ..Tokens::default()
                },
                total: Some(total),
                ..Request::default()
            })
        };
        log.take(count(10), accounts);
        log.take(count(10), accounts);
        log.take(count(20), accounts);
        let bucket = &log.parts["t1"].buckets[&BucketKey {
            hour: 0,
            model: "gpt-9-unknown".into(),
            billing: Billing::Metered,
        }];
        assert_eq!(bucket.tokens.input, 20);
        assert_eq!(bucket.unpriced, 20, "no price for it");
        assert_eq!(log.parts["t1"].cwd, "/srv/project");
    }

    #[test]
    fn a_report_joins_a_session_across_logs() {
        let mut ledger = Ledger::default();
        let accounts = &mut Accounts::default();
        let mut main = LogFile::new(Agent::Claude);
        main.take(request("s1", 3_600, Some(1), 100), accounts);
        let mut sub = LogFile::new(Agent::Claude);
        sub.take(request("s1", 3_700, Some(2), 50), accounts);
        ledger.logs.insert("/a.jsonl".into(), main);
        ledger.logs.insert("/b.jsonl".into(), sub);
        let key = SessionKey {
            agent: Agent::Claude,
            session: "s1".into(),
        };
        let report = ledger.report(&key, 0).expect("a report");
        assert_eq!(report.agent, "claude");
        assert_eq!(report.hours.len(), 1);
        assert_eq!(report.hours[0].input_tokens, 150);
        assert_eq!(ledger.last_hour(&key), 3_600);
        let other = SessionKey {
            agent: Agent::Codex,
            session: "s1".into(),
        };
        assert!(ledger.report(&other, 0).is_none());
    }
}
