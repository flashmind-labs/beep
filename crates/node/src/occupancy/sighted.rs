//! The one read of every store of the agents' sessions: what it found, who
//! asks for it and the copy a watcher keeps current while it runs.

use crate::{
    AsyncMutex,
    logs::found::{Agent, Seen},
};
use anyhow::Result;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Weak,
    },
};
use tokio::sync::watch;

/// What the agents' logs hold: the sessions, and the folders they ran in.
#[derive(Clone)]
pub(crate) struct Sighted {
    pub(crate) seen: Arc<Vec<Seen>>,
    pub(crate) folders: Arc<Vec<PathBuf>>,
}

impl Sighted {
    /// `seen`, with the folders it names.
    pub(super) fn of(seen: Vec<Seen>) -> Self {
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
    pub(super) stores: BTreeMap<Agent, Vec<Seen>>,
    pub(super) claude: Option<Vec<Seen>>,
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
    /// How many watchers hold a `Watching`. Held while a read is published
    /// and while the last watcher clears it, so a stale copy never outlives them.
    pub(super) watchers: std::sync::Mutex<usize>,
}

/// Held by a watcher that calls `Occupancy::refresh` as the stores change:
/// while any is held, `sighted` answers with what the last refresh read.
pub(crate) struct Watching(Arc<Reading>);

impl Reading {
    /// Notes a watcher that refreshes on every change, until the guard drops.
    pub(super) fn watching(self: &Arc<Self>) -> Watching {
        *crate::lock(&self.watchers) += 1;

        Watching(self.clone())
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let mut watchers = crate::lock(&self.0.watchers);
        *watchers -= 1;

        if *watchers == 0 {
            self.0.published.send_replace(None);
        }
    }
}
