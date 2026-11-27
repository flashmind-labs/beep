//! The one read of every store of the agents' sessions: who asks for it, and
//! the copy a watcher keeps current while it runs.

use super::{
    Occupancy,
    sighted::{Sighted, Watching},
};
use crate::{
    lock,
    logs::found::{Agent, Seen},
};
use anyhow::{Context, Result};
use beep_proto::watched;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Weak, atomic::Ordering},
};
use tokio::task::spawn_blocking;

impl Occupancy {
    /// Notes a watcher that refreshes on every change, until the guard drops.
    pub(crate) fn watching(&self) -> Watching {
        self.0.reading.watching()
    }

    /// The agents' logs: what the watcher last read while one runs, else a
    /// read of the stores, shared by whoever asks while it runs. One policy
    /// for listings, search and file requests.
    pub(crate) async fn sighted(&self) -> Result<Sighted> {
        if let Some(sighted) = watched::read_sent(&self.0.reading.published, Clone::clone) {
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
    pub(crate) fn reading_now(&self) -> Option<Arc<super::sighted::ReadInFlight>> {
        lock(&self.0.reading.running).upgrade()
    }
}

#[cfg(test)]
#[path = "../tests/occupancy_sighted.rs"]
mod tests;
