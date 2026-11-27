//! What the agents' logs said so far (`Ledger`): per log file how far it was
//! read, what each session spent by hour and model, and which changed. Kept
//! in one JSON file so a restart reads only what was written since.

use super::{Line, billing::Accounts};
use crate::{
    logs::found::Agent,
    logs::prices::{self, Tokens},
};
use beep_proto::SessionId;
use beep_proto::{
    AgentSlug, ModelId,
    node::usage::{Billing, HistoricalSpend, HourSpend, MAX_SESSION_HOURS, SessionSpend},
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
pub(crate) const UNKNOWN_MODEL: &str = "unknown";
/// The ledger's shape: one saved in another is dropped and the logs read
/// again. 1 marks each bucket's billing.
const VERSION: u32 = 1;

/// A session, by its agent and the agent's own id for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct SessionKey {
    pub(crate) agent: Agent,
    pub(crate) session: SessionId,
}

/// One hour of one model, paid for one way.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct BucketKey {
    /// Unix seconds at the start of the hour.
    pub(crate) hour: u64,
    pub(crate) model: ModelId,
    #[serde(default)]
    pub(crate) billing: Billing,
}

/// What was spent in one bucket.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Bucket {
    pub(crate) tokens: Tokens,
    pub(crate) cost_usd: f64,
    /// Tokens of requests no price covered.
    pub(crate) unpriced: u64,
}

impl Bucket {
    /// Adds a request: at `cost_usd` when its log said, else at the model's
    /// list price, else unpriced.
    pub(crate) fn add(&mut self, model: &str, tokens: &Tokens, cost_usd: Option<f64>) {
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
pub(crate) struct Part {
    #[serde(default)]
    pub(crate) historical: Option<HistoricalSpend>,
    pub(crate) cwd: String,
    /// A list in JSON, whose map keys are strings only.
    #[serde(with = "entries")]
    pub(crate) buckets: BTreeMap<BucketKey, Bucket>,
}

/// A file's size and modification time, which tell it changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stamp {
    pub(crate) len: u64,
    pub(crate) modified_ns: u128,
}

impl Stamp {
    pub(crate) fn of(path: &Path) -> io::Result<Self> {
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
pub(crate) struct LogFile {
    pub(crate) agent: Option<Agent>,
    pub(crate) stamp: Stamp,
    /// Read up to here: the end of the last whole line.
    pub(crate) read: u64,
    /// The session and folder its header named, for logs whose lines do not.
    pub(crate) session: Option<SessionId>,
    pub(crate) cwd: Option<String>,
    /// The model the next requests run on, for Codex.
    pub(crate) model: Option<String>,
    /// The provider its header named, for Codex.
    #[serde(default)]
    pub(crate) provider: Option<String>,
    /// Codex's running total, to drop a count it repeats.
    pub(crate) total: Option<u64>,
    /// The latest message keys, to drop Claude Code's copies.
    pub(crate) recent: VecDeque<u64>,
    pub(crate) parts: BTreeMap<SessionId, Part>,
}

impl LogFile {
    pub(crate) fn new(agent: Agent) -> Self {
        Self {
            agent: Some(agent),
            ..Self::default()
        }
    }

    /// Takes one line in, telling how its request was paid for by
    /// `accounts`; the session it added to, if any.
    pub(crate) fn take(&mut self, line: Line, accounts: &mut Accounts) -> Option<SessionId> {
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
pub(crate) struct Database {
    pub(crate) agent: Option<Agent>,
    pub(crate) stamp: Stamp,
    #[serde(default)]
    pub(crate) billed_stamp: Option<Stamp>,
    /// Older flsh caches used file modification time for undated turns.
    #[serde(default)]
    pub(crate) separates_historical_usage: bool,
    pub(crate) parts: BTreeMap<SessionId, Part>,
    /// For a flsh session file that moved here: what it had spent on the
    /// machine it came from.
    #[serde(default)]
    pub(crate) counted: Option<Counted>,
}

/// What a flsh session spent until it moved here, counted on the machine
/// it ran on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Counted {
    pub(crate) input: u64,
    pub(crate) cached: u64,
    pub(crate) output: u64,
    pub(crate) cost: f64,
}

impl Counted {
    /// What `self` has beyond `before`, and what is left of `before` after it.
    pub(crate) fn beyond(self, before: Self) -> (Self, Self) {
        let sub = |a: Self, b: Self| Self {
            input: a.input.saturating_sub(b.input),
            cached: a.cached.saturating_sub(b.cached),
            output: a.output.saturating_sub(b.output),
            cost: (a.cost - b.cost).max(0.0),
        };
        (sub(self, before), sub(before, self))
    }

    pub(crate) fn add(&mut self, other: Self) {
        self.input += other.input;
        self.cached += other.cached;
        self.output += other.output;
        self.cost += other.cost;
    }
}

/// Everything read so far.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Ledger {
    /// `VERSION` when saved; 0 from before it was kept.
    #[serde(default)]
    pub(crate) version: u32,
    pub(crate) logs: BTreeMap<PathBuf, LogFile>,
    pub(crate) databases: BTreeMap<PathBuf, Database>,
    /// When each session last changed, as a number from `revision`. A
    /// stream that heard a session at that number has it as it stands.
    #[serde(skip)]
    pub(crate) touched: BTreeMap<SessionKey, u64>,
    /// The number the latest change got.
    #[serde(skip)]
    revision: u64,
    /// Whether it changed since it was saved.
    #[serde(skip)]
    pub(crate) changed: bool,
}

impl Ledger {
    /// The ledger saved at `path`; an empty one when there is none or it
    /// cannot be read, which only means reading the logs again.
    pub(crate) fn load(path: &Path) -> Self {
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
    pub(crate) fn save(&mut self, path: &Path) -> io::Result<()> {
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

    fn all_parts(&self) -> impl Iterator<Item = (Option<Agent>, &BTreeMap<SessionId, Part>)> {
        let logs = self.logs.values().map(|log| (log.agent, &log.parts));
        let databases = self
            .databases
            .values()
            .map(|database| (database.agent, &database.parts));
        logs.chain(databases)
    }

    /// The session `key` as the caller should keep it from `since` on:
    /// every log's and database's part of it together.
    pub(crate) fn report(&self, key: &SessionKey, since: u64) -> Option<SessionSpend> {
        let parts = self
            .all_parts()
            .filter(|(agent, _)| *agent == Some(key.agent))
            .filter_map(|(_, parts)| parts.get(&key.session));
        let mut cwd = String::new();
        let mut buckets: BTreeMap<&BucketKey, Bucket> = BTreeMap::new();
        let mut historical = Vec::new();
        let mut any = false;

        for part in parts {
            any = true;
            historical.extend(part.historical.iter().cloned());

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
            agent: Into::<AgentSlug>::into(key.agent),
            session: key.session.clone(),
            cwd,
            since,
            hours,
            historical,
        })
    }

    /// Every session read so far.
    pub(crate) fn sessions(&self) -> BTreeSet<SessionKey> {
        self.all_parts()
            .filter_map(|(agent, parts)| Some((agent?, parts)))
            .flat_map(|(agent, parts)| {
                parts.keys().map(move |session| SessionKey {
                    agent,
                    session: session.clone(),
                })
            })
            .collect()
    }

    /// Notes that `session` of `agent` changed.
    pub(crate) fn touch(&mut self, agent: Agent, session: SessionId) {
        self.revision += 1;
        self.touched.insert(SessionKey { agent, session }, self.revision);
    }

    /// The latest hour `key` spent in, to send the newest sessions first.
    pub(crate) fn last_hour(&self, key: &SessionKey) -> u64 {
        self.all_parts()
            .filter_map(|(_, parts)| parts.get(&key.session))
            .filter_map(|part| {
                part.buckets
                    .keys()
                    .next_back()
                    .map(|key| key.hour)
                    .max(part.historical.as_ref().and_then(|usage| usage.last))
            })
            .max()
            .unwrap_or(0)
    }
}

/// A map as a list of its entries, for keys JSON cannot take.
mod entries {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub(crate) fn serialize<S, K, V>(map: &BTreeMap<K, V>, to: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Serialize,
        V: Serialize,
    {
        to.collect_seq(map.iter())
    }

    pub(crate) fn deserialize<'de, D, K, V>(from: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
    {
        Vec::<(K, V)>::deserialize(from).map(|entries| entries.into_iter().collect())
    }
}

#[cfg(test)]
#[path = "../../tests/logs_spend_ledger.rs"]
mod tests;
