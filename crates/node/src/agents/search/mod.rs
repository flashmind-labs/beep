//! The node's search index: the prompts and agent replies of the sessions
//! in this machine's project folders, read from the agents' own stores.
//! Phones search it with `Search`; desktops keep a copy with `FollowIndex`.

mod schedule;
mod store;

use super::{Agents, found::Seen};
use crate::link::from_mesh;
use beep_mesh::Mesh;
use beep_proto::node::agent::{EntryKind, IndexRow, SearchHit};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use schedule::{Due, Pacer, Schedule};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub use store::IndexCopy;
use store::{Indexed, Progress, Store, Walk};
use tokio::{
    sync::{Notify, broadcast, watch},
    task::spawn_blocking,
    time::{Instant, timeout},
};

const PAGE: u32 = 200;
const IN_FLIGHT: usize = 4;
/// Pages saved before this keep their progress when it runs out.
const SESSION_WAIT: Duration = Duration::from_secs(60);
/// How often the agents' stores are looked at again for new activity.
const LOOK_EVERY: Duration = Duration::from_secs(300);

pub(crate) struct SearchIndex {
    store: Mutex<Store>,
    // WAL readers keep answering while a large session is being indexed.
    reader: Mutex<Store>,
    epoch: String,
    /// The newest row, for the copies that follow.
    newest: watch::Sender<u64>,
    /// Wakes a pass before `LOOK_EVERY`.
    changed: Notify,
    schedule: Mutex<Schedule>,
    /// Sessions deleted while the node runs: a page read before the delete
    /// is not saved after it.
    deleted: Mutex<HashSet<String>>,
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
        only: Vec<String>,
    ) -> anyhow::Result<Vec<SearchHit>> {
        let index = self.clone();
        spawn_blocking(move || index.reader().find(&query, &only)).await?
    }

    /// Drops what the index holds of session `id`, as when it was deleted.
    pub async fn forget(self: &Arc<Self>, id: String) {
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

    pub async fn rows_after(self: &Arc<Self>, after: u64, most: u32) -> anyhow::Result<Vec<IndexRow>> {
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
                self.newest.send_if_modified(|was| std::mem::replace(was, seq) != seq);
            }
            Ok(Err(e)) => log::warn!("reading the search index's newest row: {e:#}"),
            Err(e) => log::warn!("reading the search index's newest row stopped: {e}"),
        }
    }

    /// The sessions to read now, and when the next of the others is due.
    fn due(&self, sessions: Vec<Seen>) -> (Vec<Seen>, Option<Instant>) {
        let (now, wall) = (Instant::now(), SystemTime::now());
        let schedule = self.schedule();
        let mut next = None;
        let mut due = Vec::new();

        for session in sessions {
            let activity = UNIX_EPOCH + Duration::from_secs(session.last_activity);
            let idle = wall.duration_since(activity).unwrap_or_default();

            match schedule.due(&session.id, activity, idle, now) {
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
pub(crate) async fn follow(mesh: Mesh, me: EndpointId, dir: PathBuf, agents: Arc<Agents>) {
    let index = match agents.search.get() {
        Some(index) => index.clone(),
        None => match SearchIndex::open(&dir.join("search.sqlite")) {
            Ok(index) => agents.search.get_or_init(|| Arc::new(index)).clone(),
            Err(e) => {
                log::warn!("no search index: {e:#}");
                return;
            }
        },
    };
    let mut pacer = Pacer::default();
    let mut wake = None;
    // The folders sessions were last dropped for. The first pass drops
    // nothing: the mesh may not have given every project yet.
    let mut dropped_for: Option<Vec<PathBuf>> = None;
    let mut first = true;

    let wakes = wake_on_changes(&index, &agents, &mesh);
    tokio::pin!(wakes);

    loop {
        let look = Instant::now() + LOOK_EVERY;
        let due = Some(wake.map_or(look, |at: Instant| at.min(look)));

        tokio::select! {
            () = pacer.next_pass(&index.changed, due) => {}
            () = &mut wakes => return,
        }


        let folders = project_folders(&mesh, me);

        // No folders yet may be a mesh still opening; once some were seen,
        // none means the last project left.
        let settled = !folders.is_empty() || dropped_for.is_some();

        if !first && settled && dropped_for.as_ref() != Some(&folders) {
            drop_outside(&index, folders.clone()).await;
            dropped_for = Some(folders.clone());
        }

        first = false;
        let Some(sessions) = in_projects(&agents, &folders).await else {
            wake = None;
            continue;
        };
        let (due, next) = index.due(sessions);
        wake = next;
        let mut work = stream::iter(due)
            .map(|session| {
                let index = index.clone();
                let agents = agents.clone();
                async move {
                    let pass = match timeout(SESSION_WAIT, index_new(&agents, &index, &session)).await {
                        Ok(Ok(true)) => Pass::Indexed,
                        Ok(Ok(false)) => Pass::Current,
                        Ok(Err(e)) => {
                            log::debug!("indexing session {}: {e:#}", session.id);
                            Pass::Failed
                        }
                        Err(_) => Pass::Partial,
                    };
                    (session, pass)
                }
            })
            .buffer_unordered(IN_FLIGHT);

        while let Some((session, pass)) = work.next().await {
            let activity = UNIX_EPOCH + Duration::from_secs(session.last_activity);

            match pass {
                Pass::Current | Pass::Indexed => index.schedule().indexed(session.id, activity),
                Pass::Partial => {
                    index.schedule().progressed(&session.id);
                    wake = sooner(wake, Instant::now());
                }
                Pass::Failed => {
                    let at = index.schedule().failed(session.id, Instant::now());
                    wake = sooner(wake, at);
                }
            }

            if matches!(pass, Pass::Indexed | Pass::Partial) {
                index.added().await;
            }
        }
    }
}

/// Wakes a pass when a session's transcript or presence changes, or the
/// mesh's projects may have; the pacer keeps passes a minute apart. Ends
/// when the node's events or the mesh close.
async fn wake_on_changes(index: &SearchIndex, agents: &Agents, mesh: &Mesh) {
    let mut events = agents.events.subscribe();
    let mut updates = mesh.updates();

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(change) if !matches!(change.topic.name(), "agents" | "*") => continue,
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

/// The sessions the agents' stores hold in `folders`, or `None` when they
/// cannot be told now.
async fn in_projects(agents: &Agents, folders: &[PathBuf]) -> Option<Vec<Seen>> {
    // An opening mesh is not a machine without projects.
    if folders.is_empty() {
        return None;
    }

    match agents.sightings().await {
        Ok(seen) => Some(
            seen.into_iter()
                .filter(|seen| folders.iter().any(|folder| seen.cwd.starts_with(folder)))
                .collect(),
        ),
        Err(e) => {
            log::warn!("listing sessions to index: {e:#}");
            None
        }
    }
}

/// The folders of the projects imported on this machine, sorted.
fn project_folders(mesh: &Mesh, me: EndpointId) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = from_mesh::placed_projects(mesh, me)
        .map(|(_, _, place)| PathBuf::from(place.folder))
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

/// Drops what the index holds of sessions outside `folders`.
async fn drop_outside(index: &Arc<SearchIndex>, folders: Vec<PathBuf>) {
    let store = index.clone();

    match spawn_blocking(move || store.store().keep_within(&folders)).await {
        Ok(Ok(0)) => {}
        Ok(Ok(_)) => {
            index.schedule().clear();
            index.added().await;
        }
        Ok(Err(e)) => log::warn!("dropping sessions from the search index: {e:#}"),
        Err(e) => log::warn!("dropping sessions from the search index stopped: {e}"),
    }
}

/// Reads the entries of `session` added since it was last indexed, newest
/// page first, and saves each page as it arrives. False when it was
/// already current.
async fn index_new(agents: &Agents, index: &Arc<SearchIndex>, session: &Seen) -> anyhow::Result<bool> {
    let stamp = session.last_activity.to_string();
    let store = index.clone();
    let (id, at) = (session.id.clone(), stamp.clone());

    if spawn_blocking(move || store.store().current(&id, &at)).await?? {
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
            cwd: session.cwd.clone(),
            activity: session.last_activity.saturating_mul(1000),
            stamp: current.then(|| stamp.clone()),
        };

        let held = index.clone();
        let saved = spawn_blocking(move || {
            let mut store = held.store();
            // Checked with the store held, so a delete is wholly before or
            // after this page.
            let deleted = crate::lock(&held.deleted).contains(&indexed.id);
            (!deleted).then(|| store.save(&indexed, &texts, progress)).transpose()
        })
        .await??;

        if saved.is_none() {
            return Ok(false);
        }

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
async fn read_page(agents: &Agents, session: &str, before: Option<u64>) -> anyhow::Result<(Vec<String>, Page)> {
    let Some((entries, earlier)) = agents.read_transcript(session, PAGE, before, false).await? else {
        anyhow::bail!("the session is gone");
    };
    let page = Page {
        earlier,
        entries: entries.len() as u64,
    };
    let texts = entries
        .into_iter()
        .filter_map(|entry| match entry.kind {
            EntryKind::User => Some(crate::agents::tags::own_words(&entry.text)),
            EntryKind::Agent => Some(entry.text),
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
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
