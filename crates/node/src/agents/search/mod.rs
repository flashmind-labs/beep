//! The node's search index: prompts and replies from this machine's
//! conversations, including the agents' own stores.
//! Phones search it with `Search`; desktops keep a copy with `FollowIndex`.

mod schedule;
mod store;

use super::Agents;
use beep_mesh::Mesh;
use beep_proto::{
    SessionId,
    node::agent::{EntryKind, IndexRow, SearchHit, SearchSession, SessionInfo},
};
use futures::{StreamExt, stream};
use schedule::{Conversation, Due, Pacer, Schedule};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
pub use store::IndexCopy;
use store::{Indexed, Progress, Store, Walk};
use tokio::{
    sync::{Notify, broadcast, watch},
    task::spawn_blocking,
    time::{Instant, timeout},
};

const PAGE: u32 = 200;
/// The most of one prompt or reply that is indexed, so a row always fits a
/// frame.
const MOST_TEXT: usize = 256 * 1024;
const IN_FLIGHT: usize = 4;
/// Pages saved before this keep their progress when it runs out.
const SESSION_WAIT: Duration = Duration::from_secs(60);

pub(crate) struct SearchIndex {
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

/// Opens the index in `dir` once, for the node's life, and keeps it up to
/// date while the mesh is open.
pub(crate) async fn follow(mesh: Mesh, dir: PathBuf, agents: Arc<Agents>) {
    let index = match agents.search.get() {
        Some(index) => index.clone(),
        None => match spawn_blocking(move || SearchIndex::open(&dir.join("search.sqlite"))).await {
            Ok(Ok(index)) => agents.search.get_or_init(|| Arc::new(index)).clone(),
            Ok(Err(e)) => {
                log::warn!("no search index: {e:#}");
                return;
            }
            Err(e) => {
                log::warn!("no search index: {e:#}");
                return;
            }
        },
    };
    // Held for the node's life: its watch rings `agent` when a store changes.
    let _stores = match agents.watch_event_sources().await {
        Ok(sources) => Some(sources),
        Err(e) => {
            log::warn!("the agents' stores are not watched for search: {e:#}");
            None
        }
    };
    let mut pacer = Pacer::default();
    let mut wake = None;
    let wakes = wake_on_changes(&index, &agents, &mesh);
    tokio::pin!(wakes);

    loop {
        tokio::select! {
            () = pacer.next_pass(&index.changed, wake) => {}
            () = &mut wakes => return,
        }

        let sessions = conversations(&agents).await;
        let (due, next) = index.due(sessions);
        wake = next;
        let mut work = stream::iter(due)
            .map(|session| {
                let index = index.clone();
                let agents = agents.clone();
                async move {
                    let mut saved = false;
                    let read = timeout(
                        SESSION_WAIT,
                        index_new(&agents, &index, &session, &mut saved),
                    )
                    .await;
                    let pass = match read {
                        Ok(Ok(true)) => Pass::Indexed,
                        Ok(Ok(false)) => Pass::Current,
                        Ok(Err(e)) => {
                            log::debug!("indexing session {}: {e:#}", session.id);
                            Pass::Failed
                        }
                        Err(_) if saved => Pass::Partial,
                        Err(_) => {
                            log::debug!("indexing session {} saved nothing in time", session.id);
                            Pass::Failed
                        }
                    };
                    (session, pass)
                }
            })
            .buffer_unordered(IN_FLIGHT);

        while let Some((session, pass)) = work.next().await {
            match pass {
                Pass::Current | Pass::Indexed => index.schedule().indexed(session),
                Pass::Partial => {
                    index.schedule().progressed(&session.id);
                    wake = sooner(wake, Instant::now());
                }
                Pass::Failed => {
                    let at = index.schedule().failed(session.id, Instant::now());
                    wake = sooner(wake, at);
                }
            }
        }

        index.added().await;
    }
}

/// Whether a change under this topic name can mean new conversation text:
/// a store written (`agent`), the session list, or everything.
fn reads_stores(topic: &str) -> bool {
    matches!(topic, "agent" | "agents" | "*")
}

/// Wakes a pass on session or mesh changes. Ends when the node's events
/// or the mesh close.
async fn wake_on_changes(index: &SearchIndex, agents: &Agents, mesh: &Mesh) {
    let mut events = agents.events.subscribe();
    let mut updates = mesh.updates();

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(change) if !reads_stores(change.topic.name()) => continue,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
            changed = updates.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }

        index.changed.notify_one();
    }
}

/// Keep the agent's id as the index key so taking over a conversation
/// changes how it opens without creating another copy of its messages.
async fn conversations(agents: &Agents) -> Vec<Conversation> {
    let stores = agents.stores().clone();
    let archived = spawn_blocking(move || stores.archived_codex());
    let (sightings, archived) = tokio::join!(agents.sightings(), archived);
    let mut seen = match sightings {
        Ok(seen) => seen,
        Err(e) => {
            log::warn!("listing sessions to index: {e:#}");
            Vec::new()
        }
    };
    match archived {
        Ok(archived) => seen.extend(archived),
        Err(e) => log::warn!("listing archived sessions to index: {e:#}"),
    }

    let owned = agents.owned();
    let mut live: HashMap<SessionId, SessionInfo> = agents
        .listed(true)
        .into_iter()
        .map(|info| (info.id.clone(), info))
        .collect();
    let mut sessions = Vec::new();

    for seen in seen {
        let managed = owned
            .get(&seen.id)
            .and_then(|id| live.remove(id))
            .or_else(|| live.remove(&seen.id));
        let last_activity = managed.as_ref().map_or(seen.last_activity, |info| {
            info.last_activity.max(seen.last_activity)
        });
        let metadata = SearchSession {
            agent: seen.agent.to_string(),
            cwd: seen.cwd.to_string_lossy().into_owned(),
            title: managed
                .as_ref()
                .and_then(|info| info.title.clone())
                .or(seen.title),
            started_at: seen.started_at,
            managed: managed.map(|info| info.id),
        };
        sessions.push(Conversation {
            id: seen.id,
            last_activity,
            metadata,
        });
    }

    sessions.extend(live.into_values().map(|info| Conversation {
        id: info.id.clone(),
        last_activity: info.last_activity.max(info.started_at),
        metadata: SearchSession {
            agent: info.agent,
            cwd: info.cwd,
            title: info.title,
            started_at: info.started_at,
            managed: Some(info.id),
        },
    }));
    sessions.sort_by_key(|session| Reverse(session.last_activity));
    let mut known = HashSet::new();
    sessions.retain(|session| known.insert(session.id.clone()));
    sessions
}

/// Reads the entries of `session` added since it was last indexed, newest
/// page first, and saves each page as it arrives, setting `saved`. False
/// when it was already current.
async fn index_new(
    agents: &Agents,
    index: &Arc<SearchIndex>,
    session: &Conversation,
    saved: &mut bool,
) -> anyhow::Result<bool> {
    let stamp = session.last_activity.to_string();
    let activity = session.last_activity.saturating_mul(1000);
    let store = index.clone();
    let (id, at) = (session.id.clone(), stamp.clone());
    let metadata = session.metadata.clone();
    let prepared = spawn_blocking(move || -> anyhow::Result<_> {
        let mut held = store.store();
        if crate::lock(&store.deleted).contains(&id) {
            return Ok((true, false));
        }

        let changed = held.set_metadata(&id, activity, &metadata)?;
        Ok((held.current(&id, &at)?, changed))
    })
    .await??;

    let (current, changed) = prepared;
    if changed {
        index.added().await;
    }
    if current {
        return Ok(false);
    }

    let store = index.clone();
    let id = session.id.clone();
    let mut progress = spawn_blocking(move || store.store().progress(&id)).await??;
    // A walk left by an earlier read finishes first; the newest page is
    // read after it.
    let mut newest = false;

    loop {
        let before = progress.walk.map(|walk| walk.low);
        let (texts, page) = read_page(agents, &session.id, before).await?;
        newest |= before.is_none();
        progress = advance(progress, page);
        let current = newest && progress.walk.is_none();
        let indexed = Indexed {
            id: session.id.clone(),
            activity,
            stamp: current.then(|| stamp.clone()),
        };

        let held = index.clone();
        let kept = spawn_blocking(move || {
            let mut store = held.store();
            // Checked with the store held, so a delete is wholly before or
            // after this page.
            let deleted = crate::lock(&held.deleted).contains(&indexed.id);
            (!deleted)
                .then(|| store.save(&indexed, &texts, progress))
                .transpose()
        })
        .await??;

        if kept.is_none() {
            return Ok(false);
        }

        *saved = true;
        index.added().await;

        if current {
            return Ok(true);
        }
    }
}

/// Where a transcript page starts and how many entries it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Page {
    earlier: u64,
    entries: u64,
}

/// The user's prompts and the agent's replies on the page before entry
/// `before` (the newest page when `None`).
async fn read_page(
    agents: &Agents,
    session: &SessionId,
    before: Option<u64>,
) -> anyhow::Result<(Vec<String>, Page)> {
    let Some((entries, earlier)) = agents.read_transcript(session, PAGE, before, false).await?
    else {
        anyhow::bail!("the session is gone");
    };
    let page = Page {
        earlier,
        entries: entries.len() as u64,
    };
    let texts = entries
        .into_iter()
        .filter_map(|entry| match entry.kind {
            EntryKind::User => Some(crate::tags::own_words(&entry.text)),
            EntryKind::Agent => Some(entry.text),
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
        .map(|mut text| {
            text.truncate(text.floor_char_boundary(MOST_TEXT));
            text
        })
        .collect();

    Ok((texts, page))
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
