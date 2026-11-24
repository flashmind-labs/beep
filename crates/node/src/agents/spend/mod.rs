//! What coding agent sessions on this machine spent (`Spend`), read from the
//! agents' own logs, sessions started outside Beep included, for the caller's
//! `FollowUsage` stream.

use beep_proto::SessionId;
use beep_proto::node::usage::{Billing, SessionSpend};
use ledger::{Ledger, SessionKey};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

mod agy;
mod billing;
mod claude;
mod codex;
mod databases;
mod ledger;
mod pi;
mod scan;

pub(crate) use pi::session_cost as pi_session_cost;
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
        id: SessionId,
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
    session: Option<SessionId>,
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

/// What one `FollowUsage` stream has heard: each session at the number of
/// its latest change.
#[derive(Debug, Default)]
pub(crate) struct Heard(BTreeMap<SessionKey, u64>);

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
    /// the sessions `heard` has not heard of as they stand, the newest
    /// first, then notes them in `heard`. Each stream keeps its own `heard`,
    /// so none takes news from another. Blocking: run it with
    /// `spawn_blocking`. A session is reported whole from the window's
    /// start, so one read again replaces itself on the caller rather than
    /// adding to it.
    pub(crate) fn pass(&self, now: u64, heard: &mut Heard) -> Vec<SessionSpend> {
        // On an hour: the caller replaces each reported hour whole, so a
        // window starting within one would shrink it to what came after.
        let since = now.saturating_sub(WINDOW_SECS) / 3600 * 3600;
        // Held across the read: only passes and `mark_counted` take it, and
        // all of them run on the blocking pool.
        let mut guard = lock(&self.ledger);
        let ledger = self.loaded(&mut guard);
        scan::pass(ledger, &self.roots, since);

        if let Some(path) = &self.path
            && let Err(e) = ledger.save(path)
        {
            log::warn!(
                "agent usage ledger not saved; path={} error={e}",
                path.display()
            );
        }

        let mut news: Vec<(u64, SessionKey, u64)> = ledger
            .sessions()
            .into_iter()
            .filter_map(|key| {
                let at = ledger.touched.get(&key).copied().unwrap_or(0);
                (heard.0.get(&key) != Some(&at)).then(|| (ledger.last_hour(&key), key, at))
            })
            .collect();
        news.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        news.into_iter()
            .take(MAX_REPORTS)
            .filter_map(|(_, key, at)| {
                let report = ledger.report(&key, since);
                heard.0.insert(key, at);
                report
            })
            .collect()
    }

    /// The ledger, read from `path` the first time.
    fn loaded<'a>(&self, guard: &'a mut Option<Ledger>) -> &'a mut Ledger {
        guard.get_or_insert_with(|| match &self.path {
            Some(path) => Ledger::load(path),
            None => Ledger::default(),
        })
    }

    fn save(&self, ledger: &mut Ledger) -> std::io::Result<()> {
        match &self.path {
            Some(path) => ledger.save(path),
            None => Ok(()),
        }
    }

    /// Notes that what the log at `path` holds now was spent elsewhere: a
    /// session moved here with its transcript, and the machine it ran on
    /// already counted it. Only what is appended from here on counts.
    pub(crate) fn mark_counted(
        &self,
        agent: Agent,
        session: &SessionId,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        let stamp = ledger::Stamp::of(path)?;

        if agent == Agent::Flsh {
            // flsh keeps totals, not a log: what they say now is held back.
            let counted = databases::flsh_counted(path)?;
            let mut guard = lock(&self.ledger);
            let ledger = self.loaded(&mut guard);
            let entry = ledger.databases.entry(path.to_owned()).or_default();
            entry.agent = Some(agent);
            entry.stamp = stamp;
            entry.counted = counted;
            ledger.changed = true;
            return self.save(ledger);
        }
        // What the header says (the folder, the provider, the model) is
        // read from the log, so later requests are billed and filed as the
        // log's own would be.
        let header = scan::header_of(agent, path, &self.roots.accounts)?;
        let mut guard = lock(&self.ledger);
        let ledger = self.loaded(&mut guard);
        // A session that came back here keeps what it spent here before: its
        // log is read on from its new end, not started over.
        let log = ledger.logs.entry(path.to_owned()).or_default();
        log.agent = Some(agent);
        log.stamp = stamp;
        log.read = stamp.len;
        log.session = Some(session.to_owned());
        log.cwd = header.cwd.or(log.cwd.take());
        log.model = header.model.or(log.model.take());
        log.provider = header.provider.or(log.provider.take());
        log.total = header.total;
        log.recent = header.recent;
        ledger.changed = true;

        self.save(ledger)
    }
}

#[cfg(test)]
#[path = "../../tests/agents_spend.rs"]
mod tests;
