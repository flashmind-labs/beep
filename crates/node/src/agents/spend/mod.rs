//! What coding agent sessions on this machine spent (`Spend`), read from the
//! agents' own logs, sessions started outside Beep included, for the caller's
//! `FollowUsage` stream.

use beep_proto::node::usage::SessionSpend;
use beep_proto::{AgentSlug, SessionId};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};
use tokio::{
    sync::{
        broadcast,
        broadcast::error::{RecvError, TryRecvError},
        watch,
    },
    task::spawn_blocking,
    time::{Instant, sleep_until},
};
use tokio_util::task::AbortOnDropHandle;

pub(crate) use crate::logs::spend::pi_session_cost;
use crate::logs::spend::{
    Roots, databases,
    ledger::{Ledger, SessionKey, Stamp},
    scan,
};

use crate::{agents::limits::SPEND_GAP, events::Change, lock, logs::found::Agent, logs::unix_now};

/// How far back the logs are read: a month and a bit.
const WINDOW_SECS: u64 = 35 * 24 * 3600;
/// The most sessions one pass reports.
const MAX_REPORTS: usize = 500;

/// What the scan has reported of each session, as its latest report stands.
pub(crate) type Reports = BTreeMap<(AgentSlug, SessionId), SessionSpend>;

/// The task that reads the logs while any stream follows them. The last
/// follower to drop it ends the task.
#[derive(Debug)]
pub(crate) struct Scan {
    _task: AbortOnDropHandle<()>,
}

/// Tells the scan the agents' logs changed: the rings of the agents' event
/// hub, while the file watcher behind them runs.
pub(crate) struct LogChanges {
    rings: broadcast::Receiver<Change>,
    _watching: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

impl LogChanges {
    /// Follows `rings`, keeping `watching` (the watcher that makes them) alive.
    pub(crate) fn new(
        rings: broadcast::Receiver<Change>,
        watching: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Self {
        Self {
            rings,
            _watching: watching,
        }
    }

    /// Returns once a log changed since the last call; rings missed count.
    async fn changed(&mut self) {
        loop {
            match self.rings.recv().await {
                Ok(change) if change.topic.name() != "agent" && change.topic.name() != "*" => {}
                Ok(_) | Err(RecvError::Lagged(_)) => return,
                // The hub lives as long as the node.
                Err(RecvError::Closed) => std::future::pending::<()>().await,
            }
        }
    }

    /// Forgets the rings so far: a read that starts now covers them.
    fn settle(&mut self) {
        while !matches!(
            self.rings.try_recv(),
            Err(TryRecvError::Empty | TryRecvError::Closed)
        ) {}
    }
}

/// What the scan has reported to the scan's followers: each session at the
/// number of its latest change.
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
    /// What the passes reported, for every stream that follows.
    reports: watch::Sender<Reports>,
    /// The scan while some stream holds it. Held only to join or start one.
    scan: Mutex<Weak<Scan>>,
}

impl Spend {
    /// The logs on this machine, the ledger in the node's cache folder.
    pub(crate) fn locate() -> Self {
        Self {
            roots: Roots::locate(),
            path: dirs::cache_dir().map(|dir| dir.join("beep-node").join("agent-spend.json")),
            ledger: Mutex::default(),
            reports: watch::Sender::default(),
            scan: Mutex::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn at(roots: Roots, path: PathBuf) -> Self {
        Self {
            roots,
            path: Some(path),
            ledger: Mutex::default(),
            reports: watch::Sender::default(),
            scan: Mutex::default(),
        }
    }

    /// Follows what sessions spent. One task reads the logs when `logs` says
    /// they changed, at most every `SPEND_GAP`, for all followers while any
    /// holds the returned `Scan`; the reports are as the last pass left them.
    pub(crate) fn follow(
        self: &Arc<Self>,
        logs: LogChanges,
    ) -> (Arc<Scan>, watch::Receiver<Reports>) {
        let mut running = lock(&self.scan);
        let scan = running.upgrade().unwrap_or_else(|| {
            let scan = Arc::new(Scan {
                _task: AbortOnDropHandle::new(tokio::spawn(self.clone().scan_on(logs))),
            });
            *running = Arc::downgrade(&scan);
            scan
        });

        (scan, self.reports.subscribe())
    }

    /// Reads the logs once, then whenever they change, and publishes what
    /// changed. A pass runs on the blocking pool: dropped with the task, it
    /// still finishes and saves what it read.
    async fn scan_on(self: Arc<Self>, mut logs: LogChanges) {
        let mut heard = Heard::default();
        let mut next = Instant::now();

        loop {
            sleep_until(next).await;
            logs.settle();
            next = Instant::now() + SPEND_GAP;
            let spend = self.clone();
            let read = spawn_blocking(move || {
                let reports = spend.pass(unix_now(), &mut heard);
                (heard, reports)
            })
            .await;

            match read {
                Ok((known, reports)) => {
                    heard = known;
                    self.publish(reports);
                }
                Err(e) => {
                    log::warn!("reading the agents' logs failed: {e}");
                    heard = Heard::default();
                }
            }

            logs.changed().await;
        }
    }

    fn publish(&self, reports: Vec<SessionSpend>) {
        self.reports.send_if_modified(|all| {
            let mut changed = false;

            for report in reports {
                let key = (report.agent.clone(), report.session.clone());

                if all.get(&key) != Some(&report) {
                    all.insert(key, report);
                    changed = true;
                }
            }

            changed
        });
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
        let stamp = Stamp::of(path)?;

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
