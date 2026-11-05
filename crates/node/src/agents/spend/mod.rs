//! What coding agent sessions on this machine spent (`Spend`), read from the
//! agents' own logs, sessions started outside Beep included, for the server's
//! `FollowUsage` stream.

use beep_proto::node::usage::{Billing, SessionSpend};
use ledger::{Ledger, SessionKey};
use std::{path::PathBuf, sync::Mutex};

mod billing;
mod claude;
mod codex;
mod databases;
mod ledger;
mod pi;
mod scan;

pub(crate) use scan::Roots;

use crate::{agents::found::Agent, agents::prices::Tokens, lock};

/// How far back the logs are read: a month and a bit.
const WINDOW_SECS: u64 = 35 * 24 * 3600;
/// The most sessions one pass reports.
const MAX_REPORTS: usize = 500;

/// What one log line said.
#[derive(Debug)]
pub(super) enum Line {
    /// The session the log is of, from its header, and the provider it
    /// runs on when the header names one.
    Session {
        id: String,
        cwd: String,
        provider: Option<String>,
    },
    /// The model the next requests run on.
    Model(String),
    /// One request to the model.
    Request(Request),
}

/// One request and its tokens, as one log line records it.
#[derive(Debug, Default)]
pub(super) struct Request {
    /// The session, when the line names it rather than the log's header.
    session: Option<String>,
    cwd: Option<String>,
    /// Unix seconds.
    at: u64,
    model: Option<String>,
    tokens: Tokens,
    /// What it cost, when the log says.
    cost_usd: Option<f64>,
    /// Lines with the same key are copies of one request.
    key: Option<u64>,
    /// A running total that a repeated count repeats.
    total: Option<u64>,
    /// The provider it went to, when the line names it.
    provider: Option<String>,
    /// How it was paid for, when the line itself tells.
    billing: Billing,
}

/// The logs and what was read of them.
#[derive(Debug, Default)]
pub(crate) struct Spend {
    roots: Roots,
    /// Where the ledger is saved; nowhere when `None`.
    path: Option<PathBuf>,
    /// Read from `path` at the first pass.
    ledger: Mutex<Option<Ledger>>,
}

impl Spend {
    /// The logs on this machine, the ledger in the node's cache folder.
    pub(crate) fn locate() -> Self {
        Self {
            roots: Roots::locate(),
            path: dirs::cache_dir().map(|dir| dir.join("beep-node").join("agent-spend.json")),
            ledger: Mutex::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn at(roots: Roots, path: PathBuf) -> Self {
        Self {
            roots,
            path: Some(path),
            ledger: Mutex::default(),
        }
    }

    /// Reads what changed in the logs by `now` (Unix seconds), and gives
    /// the sessions the server has not heard of as they stand, the newest
    /// first. Blocking: run it with `spawn_blocking`. A session is reported
    /// whole from the window's start, so one read again replaces itself on
    /// the server rather than adding to it.
    pub(crate) fn pass(&self, now: u64) -> Vec<SessionSpend> {
        self.read(now, false)
    }

    /// As `pass`, with every session as it stands, heard of or not: what a
    /// new stream starts from, as its reader may keep nothing. It leaves the
    /// unsent sessions as they are, for a stream that started earlier.
    pub(crate) fn pass_all(&self, now: u64) -> Vec<SessionSpend> {
        self.read(now, true)
    }

    fn read(&self, now: u64, all: bool) -> Vec<SessionSpend> {
        // On an hour: the server replaces each reported hour whole, so a
        // window starting within one would shrink it to what came after.
        let since = now.saturating_sub(WINDOW_SECS) / 3600 * 3600;
        let mut guard = lock(&self.ledger);
        let ledger = guard.get_or_insert_with(|| match &self.path {
            Some(path) => Ledger::load(path),
            None => Ledger::default(),
        });
        scan::pass(ledger, &self.roots, since);
        forget_gone(ledger, since);
        if let Some(path) = &self.path
            && let Err(e) = ledger.save(path)
        {
            log::warn!(
                "agent usage ledger not saved; path={} error={e}",
                path.display()
            );
        }
        let sessions = if all {
            ledger.sessions()
        } else {
            ledger.unsent.clone()
        };
        let mut keys: Vec<(u64, &SessionKey)> = sessions
            .iter()
            .map(|key| (ledger.last_hour(key), key))
            .collect();
        keys.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        keys.into_iter()
            .take(MAX_REPORTS)
            .filter_map(|(_, key)| ledger.report(key, since))
            .collect()
    }

    /// Notes that what the log at `path` holds now was spent elsewhere: a
    /// session moved here with its transcript, and the machine it ran on
    /// already counted it. Only what is appended from here on counts.
    pub(crate) fn mark_counted(
        &self,
        agent: Agent,
        session: &str,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        let stamp = ledger::Stamp::of(path)?;
        let mut guard = lock(&self.ledger);
        let ledger = guard.get_or_insert_with(|| match &self.path {
            Some(path) => Ledger::load(path),
            None => Ledger::default(),
        });
        let mut log = ledger::LogFile::new(agent);
        log.stamp = stamp;
        log.read = stamp.len;
        log.session = Some(session.to_owned());
        ledger.logs.insert(path.to_owned(), log);
        ledger.changed = true;
        match &self.path {
            Some(saved) => ledger.save(saved),
            None => Ok(()),
        }
    }

    /// Notes that the server has `spend` as it stands; it is sent again
    /// only once it changes.
    pub(crate) fn sent(&self, spend: &SessionSpend) {
        let Some(agent) = Agent::from_name(spend.agent.as_str()) else {
            return;
        };
        let mut guard = lock(&self.ledger);
        if let Some(ledger) = guard.as_mut() {
            let key = SessionKey {
                agent,
                session: spend.session.clone(),
            };
            if ledger.unsent.remove(&key) {
                ledger.changed = true;
            }
        }
    }
}

/// Forgets the unsent sessions whose logs are all gone: there is nothing
/// to send of them.
fn forget_gone(ledger: &mut Ledger, since: u64) {
    let gone: Vec<SessionKey> = ledger
        .unsent
        .iter()
        .filter(|key| ledger.report(key, since).is_none())
        .cloned()
        .collect();
    for key in gone {
        ledger.unsent.remove(&key);
        ledger.changed = true;
    }
}

#[cfg(test)]
mod tests;
