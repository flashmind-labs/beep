//! The one read of every store of the agents' sessions: what it found, who
//! asks for it and the copy a watcher keeps current while it runs.

use super::Occupancy;
use crate::{
    AsyncMutex, lock,
    logs::found::{Agent, Seen},
};
use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{sync::watch, task::spawn_blocking};

/// What the agents' logs hold: the sessions, and the folders they ran in.
#[derive(Clone)]
pub(crate) struct Sighted {
    pub(crate) seen: Arc<Vec<Seen>>,
    pub(crate) folders: Arc<Vec<PathBuf>>,
}

impl Sighted {
    /// `seen`, with the folders it names.
    fn of(seen: Vec<Seen>) -> Self {
        let mut folders: Vec<PathBuf> = seen.iter().map(|seen| seen.cwd.clone()).collect();
        folders.sort();
        folders.dedup();

        Self {
            seen: Arc::new(seen),
            folders: Arc::new(folders),
        }
    }
}

/// The outcome of one read of every store, set once by the caller that runs it.
pub(super) type ReadInFlight = tokio::sync::OnceCell<Result<Sighted, Arc<anyhow::Error>>>;

/// What the last read of each store listed, kept for the stores that did
/// not change.
#[derive(Default)]
pub(super) struct Listings {
    stores: BTreeMap<Agent, Vec<Seen>>,
    claude: Option<Vec<Seen>>,
}

/// What `Occupancy::sighted` publishes and counts.
#[derive(Default)]
pub(super) struct Reading {
    /// The last read while a watcher runs, none before its first.
    pub(super) published: watch::Sender<Option<Sighted>>,
    /// The read of every store now running, shared by whoever asks while it
    /// does. Held only to join or start one, never across a read.
    pub(super) running: std::sync::Mutex<Weak<ReadInFlight>>,
    pub(super) listings: AsyncMutex<Listings>,
    pub(super) watchers: AtomicUsize,
}

/// Held by a watcher that calls `Occupancy::refresh` as the stores change:
/// while any is held, `sighted` answers with what the last refresh read.
pub(crate) struct Watching(Occupancy);

impl Drop for Watching {
    fn drop(&mut self) {
        if self.0.0.reading.watchers.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.0.reading.published.send_replace(None);
        }
    }
}

impl Occupancy {
    /// Notes a watcher that refreshes on every change, until the guard drops.
    pub(crate) fn watching(&self) -> Watching {
        self.0.reading.watchers.fetch_add(1, Ordering::SeqCst);

        Watching(self.clone())
    }

    /// The agents' logs: what the watcher last read while one runs, else a
    /// read of the stores, shared by whoever asks while it runs. One policy
    /// for listings, search and file requests.
    pub(crate) async fn sighted(&self) -> Result<Sighted> {
        if let Some(sighted) = self.0.reading.published.borrow().clone() {
            return Ok(sighted);
        }

        let running = {
            let mut slot = lock(&self.0.reading.running);

            match slot.upgrade() {
                Some(running) => running,
                None => {
                    let running = Arc::default();
                    *slot = Arc::downgrade(&running);
                    running
                }
            }
        };

        running
            .get_or_init(|| async {
                let all: BTreeSet<Agent> = Agent::ALL.into_iter().collect();
                self.refresh(&all).await.map_err(Arc::new)
            })
            .await
            .clone()
            .map_err(|error| anyhow::anyhow!("{error:#}"))
    }

    /// Reads the stores again, those of `changed` anew and the others as the
    /// last read left them, and publishes the result to `sighted` while a
    /// watcher runs.
    pub(crate) async fn refresh(&self, changed: &BTreeSet<Agent>) -> Result<Sighted> {
        let read = self.read_stores(changed).await;

        if self.0.reading.watchers.load(Ordering::SeqCst) > 0 {
            self.0
                .reading
                .published
                .send_replace(read.as_ref().ok().cloned());
        }

        read
    }

    /// Forgets the read now running, so the next ask starts its own: one that
    /// began before a change still lists what the change removed.
    pub(crate) fn forget_reading(&self) {
        *lock(&self.0.reading.running) = Weak::new();
    }

    async fn read_stores(&self, changed: &BTreeSet<Agent>) -> Result<Sighted> {
        metrics::counter!("beep_store_reads_total").increment(1);

        let mut cache = self.0.reading.listings.lock().await;
        let stores = self.stores().clone();
        let mut listings = std::mem::take(&mut cache.stores);
        let stale = changed.clone();
        let stored = spawn_blocking(move || {
            for agent in Agent::STORED {
                if stale.contains(&agent) || !listings.contains_key(&agent) {
                    listings.insert(agent, stores.sessions(Some(agent)));
                }
            }

            listings
        });
        let refresh_claude = cache.claude.is_none() || changed.contains(&Agent::Claude);
        let claude = async {
            if refresh_claude {
                Some(self.claude().sightings().await)
            } else {
                None
            }
        };
        let (claude, stored) = tokio::join!(claude, stored);
        cache.stores = stored.context("reading the agents' sessions")?;

        match claude {
            Some(Ok(seen)) => cache.claude = Some(seen),
            Some(Err(e)) => {
                log::warn!("Claude Code sessions not read: {e:#}");
                cache.claude = None;
            }
            None => {}
        }

        let mut seen = cache.claude.clone().unwrap_or_default();

        for agent in Agent::STORED {
            seen.extend(cache.stores[&agent].iter().cloned());
        }

        Ok(Sighted::of(seen))
    }

    /// `seen` with the sessions known only from a live connection.
    pub(crate) fn with_present(&self, mut seen: Vec<Seen>) -> Vec<Seen> {
        for native in self.presence().snapshot() {
            if seen
                .iter()
                .any(|seen| seen.agent.as_ref() == native.agent && seen.id == native.id)
            {
                continue;
            }

            seen.push(Seen {
                agent: Agent::Flsh,
                id: native.id,
                cwd: PathBuf::from(native.cwd),
                started_at: native.started_at,
                last_activity: native.last_activity,
                title: native.title,
                file: None,
            });
        }

        seen
    }

    /// The read of every store now running, for a test.
    #[cfg(test)]
    pub(crate) fn reading_now(&self) -> Option<Arc<ReadInFlight>> {
        lock(&self.0.reading.running).upgrade()
    }
}
