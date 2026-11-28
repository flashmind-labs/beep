//! The node's search index: prompts and replies from this machine's
//! conversations, including the agents' own stores.
//! Phones search it with `Search`; desktops keep a copy with `FollowIndex`.

mod follow;
mod schedule;
mod store;

pub(crate) use follow::follow;

use crate::{
    agents::{follow::sources::EventBridge, listings::Listings, sessions::Sessions},
    events::Hub,
};
use beep_proto::{
    SessionId,
    node::agent::{IndexRow, SearchHit},
};
use schedule::{Conversation, Due, Schedule};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
    time::Duration,
};
pub use store::IndexCopy;
use store::{Progress, Store, Walk};
use tokio::{
    sync::{Notify, watch},
    task::spawn_blocking,
    time::Instant,
};

const PAGE: u32 = 200;
/// The most of one prompt or reply that is indexed, so a row always fits a
/// frame.
const MOST_TEXT: usize = 256 * 1024;
const IN_FLIGHT: usize = 4;
/// Pages saved before this keep their progress when it runs out.
const SESSION_WAIT: Duration = Duration::from_secs(60);

/// Owns whether the node has a search index: opened when the mesh is
/// joined, and tried again on the next join if the file would not open.
pub(crate) struct Opener {
    path: Option<PathBuf>,
    index: OnceLock<Arc<SearchIndex>>,
    /// What the follow reads to index conversations.
    listings: Arc<Listings>,
    sessions: Arc<Sessions>,
    bridge: Arc<EventBridge>,
    events: Hub,
}

impl Opener {
    pub(crate) fn new(
        path: Option<PathBuf>,
        listings: Arc<Listings>,
        sessions: Arc<Sessions>,
        bridge: Arc<EventBridge>,
        events: Hub,
    ) -> Self {
        Self {
            path,
            index: OnceLock::new(),
            listings,
            sessions,
            bridge,
            events,
        }
    }

    /// The index, once it is open.
    pub(crate) fn get(&self) -> Option<Arc<SearchIndex>> {
        self.index.get().cloned()
    }

    /// Opens the index if it is not open yet. `None` when there is no file
    /// to open or it would not open now.
    pub(crate) async fn open(&self) -> Option<Arc<SearchIndex>> {
        if let Some(index) = self.get() {
            return Some(index);
        }

        let path = self.path.clone()?;

        match spawn_blocking(move || SearchIndex::open(&path)).await {
            Ok(Ok(index)) => Some(self.index.get_or_init(|| Arc::new(index)).clone()),
            Ok(Err(e)) => {
                log::warn!("no search index: {e:#}");
                None
            }
            Err(e) => {
                log::warn!("no search index: {e:#}");
                None
            }
        }
    }
}

pub struct SearchIndex {
    store: Mutex<Store>,
    // WAL readers keep answering while a large session is being indexed.
    reader: Mutex<Store>,
    epoch: String,
    /// The newest row, for the copies that follow.
    newest: watch::Sender<u64>,
    /// Wakes a pass.
    changed: Notify,
    schedule: Mutex<Schedule>,
    /// Sessions deleted while the node runs: a page read before the delete
    /// is not saved after it.
    deleted: Mutex<HashSet<SessionId>>,
}

/// What one read of a session did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pass {
    Current,
    Indexed,
    /// Stopped after saving some pages: the next pass goes on.
    Partial,
    Failed,
}

impl SearchIndex {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let store = Store::open(path)?;
        let reader = Store::open(path)?;
        let epoch = store.epoch()?;
        let newest = reader.newest()?;
        Ok(Self {
            store: Mutex::new(store),
            reader: Mutex::new(reader),
            epoch,
            newest: watch::Sender::new(newest),
            changed: Notify::new(),
            schedule: Mutex::default(),
            deleted: Mutex::default(),
        })
    }

    pub fn epoch(&self) -> &str {
        &self.epoch
    }

    /// Changes each time rows are added, to the newest one.
    pub fn newest(&self) -> watch::Receiver<u64> {
        self.newest.subscribe()
    }

    /// The sessions holding every word of `query`; only those of `only`
    /// when it names any.
    pub async fn find(
        self: &Arc<Self>,
        query: String,
        only: Vec<SessionId>,
    ) -> anyhow::Result<Vec<SearchHit>> {
        let index = self.clone();
        spawn_blocking(move || index.reader().find(&query, &only)).await?
    }

    /// Drops what the index holds of session `id`, as when it was deleted.
    pub async fn forget(self: &Arc<Self>, id: SessionId) {
        // Marked before the store is reset: a save that waits for the store
        // after this sees it.
        crate::lock(&self.deleted).insert(id.clone());
        let index = self.clone();

        match spawn_blocking(move || index.store().forget(&id)).await {
            Ok(Ok(())) => self.added().await,
            Ok(Err(e)) => log::warn!("dropping a session from the search index: {e:#}"),
            Err(e) => log::warn!("dropping a session from the search index stopped: {e}"),
        }
    }

    pub async fn rows_after(
        self: &Arc<Self>,
        after: u64,
        most: u32,
    ) -> anyhow::Result<Vec<IndexRow>> {
        let index = self.clone();
        spawn_blocking(move || index.reader().rows_after(after, most)).await?
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reader(&self) -> MutexGuard<'_, Store> {
        self.reader.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn schedule(&self) -> MutexGuard<'_, Schedule> {
        self.schedule.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Tells the copies about rows added since the last call.
    async fn added(self: &Arc<Self>) {
        let index = self.clone();

        match spawn_blocking(move || index.store().newest()).await {
            Ok(Ok(seq)) => {
                self.newest
                    .send_if_modified(|was| std::mem::replace(was, seq) != seq);
            }
            Ok(Err(e)) => log::warn!("reading the search index's newest row: {e:#}"),
            Err(e) => log::warn!("reading the search index's newest row stopped: {e}"),
        }
    }

    /// The sessions to read now, and when the next of the others is due.
    fn due(&self, sessions: Vec<Conversation>) -> (Vec<Conversation>, Option<Instant>) {
        let now = Instant::now();
        let schedule = self.schedule();
        let mut next = None;
        let mut due = Vec::new();

        for session in sessions {
            match schedule.due(&session, now) {
                Due::Current => {}
                Due::At(at) => next = sooner(next, at),
                Due::Now => due.push(session),
            }
        }

        (due, next)
    }
}

/// Where a transcript page starts and how many entries it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Page {
    earlier: u64,
    entries: u64,
}

/// How far the session is indexed once `page` is.
fn advance(progress: Progress, page: Page) -> Progress {
    let Some(walk) = progress.walk else {
        // The newest page. Where it ends is at worst a page past what was
        // read before, so a page may be read twice.
        let end = page.earlier + page.entries;
        // A transcript shorter than what was indexed was numbered again,
        // such as a large one read from a later point: read it all again.
        // Texts already kept are not kept twice.
        let progress = if end < progress.done {
            Progress::NEW
        } else {
            progress
        };

        return if page.earlier <= progress.done {
            Progress {
                done: progress.done.max(end),
                walk: None,
            }
        } else {
            Progress {
                done: progress.done,
                walk: Some(Walk {
                    top: end.max(progress.done),
                    low: page.earlier,
                }),
            }
        };
    };

    // A store that cannot read before an entry answers with its newest
    // page again, and past entries a session dropped the answer is empty:
    // either way nothing older can be read.
    let reached = !valid_page(walk.low, page) || page.earlier <= progress.done;

    if reached {
        return Progress {
            done: walk.top,
            walk: None,
        };
    }

    Progress {
        walk: Some(Walk {
            low: page.earlier,
            ..walk
        }),
        ..progress
    }
}

fn valid_page(before: u64, page: Page) -> bool {
    page.earlier < before
        && page
            .earlier
            .checked_add(page.entries)
            .is_some_and(|end| end <= before)
}

fn sooner(wake: Option<Instant>, at: Instant) -> Option<Instant> {
    Some(wake.map_or(at, |wake| wake.min(at)))
}

#[cfg(test)]
#[path = "../../tests/agents_search.rs"]
mod tests;
