//! Background prompt indexing over the existing transcript protocol. The
//! menu reads SQLite, never a machine; unchanged sessions need no reread.

mod schedule;
mod store;

use super::{index::Entry, lists::PaletteCache, places::Place, projects::session_entry};
use crate::{
    error::Result,
    fleet::{AsyncMutex, Fleet},
};
use anyhow::{Context, bail};
use beep_mesh::machine::sessions::{Reading, Sessions};
use beep_node::agents::tags::own_words;
use beep_proto::{
    ImportedProject, MachineSession,
    node::agent::{EntryKind, SessionsReply, SessionsRequest},
};
use futures::{StreamExt, stream};
use schedule::{Due, Pacer, Schedule};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};
use store::{Progress, Store, Walk, identity, stamp};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{
    sync::Notify,
    task::spawn_blocking,
    time::{Instant, timeout},
};

const PAGE: u32 = 200;
const IN_FLIGHT: usize = 4;
/// Pages saved before this keep their progress when it runs out.
const SESSION_WAIT: Duration = Duration::from_secs(60);

pub(crate) struct PromptIndex {
    store: Mutex<Store>,
    // WAL readers keep answering while a large session is being indexed.
    reader: Option<Mutex<Store>>,
    changed: Notify,
    project_changes: AsyncMutex<()>,
    /// When each session is read next, so an unchanged session is skipped
    /// without asking the store.
    schedule: Mutex<Schedule>,
}

/// What one pass did with a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pass {
    Current,
    Indexed,
    /// Stopped after saving some pages: the next pass goes on.
    Partial,
    /// Its machine is offline: read when a pass finds it online.
    Offline,
    Failed,
}

impl PromptIndex {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let (store, reader) =
            match Store::open(path).and_then(|store| Ok((store, Some(Store::open(path)?)))) {
                Ok(pair) => pair,
                Err(e) => {
                    // A derived index must never stop the app opening.
                    log::warn!("starting without the saved prompt index: {e:#}");
                    (Store::open(Path::new(":memory:"))?, None)
                }
            };
        Ok(Self {
            store: Mutex::new(store),
            reader: reader.map(Mutex::new),
            changed: Notify::new(),
            project_changes: AsyncMutex::new(()),
            schedule: Mutex::default(),
        })
    }

    fn schedule(&self) -> MutexGuard<'_, Schedule> {
        self.schedule.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The sessions to read now, and when the next of the others is due.
    fn due(&self, sessions: Vec<MachineSession>) -> (Vec<MachineSession>, Option<Instant>) {
        let (now, wall) = (Instant::now(), SystemTime::now());
        let schedule = self.schedule();
        let mut next = None;
        let mut due = Vec::new();

        for session in sessions {
            let idle = wall
                .duration_since(session.last_activity)
                .unwrap_or_default();

            match schedule.due(&identity(&session), session.last_activity, idle, now) {
                Due::Current => {}
                Due::At(at) => next = sooner(next, at),
                Due::Now => due.push(session),
            }
        }

        (due, next)
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reader(&self) -> MutexGuard<'_, Store> {
        self.reader
            .as_ref()
            .unwrap_or(&self.store)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) async fn project_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.project_changes.lock().await
    }

    pub(crate) async fn sync_projects(
        self: &Arc<Self>,
        projects: Vec<ImportedProject>,
    ) -> anyhow::Result<Vec<Place>> {
        let index = self.clone();
        let removed = spawn_blocking(move || index.store().sync_projects(&projects)).await??;

        if !removed.is_empty() {
            self.schedule().clear();
        }

        Ok(removed)
    }

    pub(crate) async fn forget_project(
        self: &Arc<Self>,
        app: &AppHandle,
        project: &str,
    ) -> anyhow::Result<()> {
        let known = app.state::<PaletteCache>().projects();
        let index = self.clone();
        let name = project.to_owned();
        let removed = spawn_blocking(move || index.store().remove_project(&name, &known)).await??;
        self.schedule().clear();
        app.state::<PaletteCache>()
            .forget_project(app, project, removed);
        let _ = app.emit("palette-text-changed", ());
        Ok(())
    }

    pub(super) fn changed(&self) {
        self.changed.notify_one();
    }

    pub(crate) async fn remove(self: &Arc<Self>, id: String) {
        self.schedule().forget(&id);
        let index = self.clone();
        let removed = async {
            spawn_blocking(move || index.store().remove(&id)).await??;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = removed {
            log::warn!("removing a session from the prompt index: {e}");
        }
    }
}

/// Called only after the node accepted the message. Indexing
/// failures do not turn a successful send into an apparent failed send.
pub(crate) async fn sent(app: &AppHandle, session: MachineSession, text: &str) {
    let text = own_words(text);
    let text = text.trim().to_owned();

    if text.is_empty() {
        return;
    }

    let index = app.state::<Arc<PromptIndex>>().inner().clone();
    let saved = index.clone();
    let listed = session.clone();

    match spawn_blocking(move || saved.store().append(&listed, &text)).await {
        Ok(Ok(true)) => {
            app.state::<PaletteCache>().remember_session(app, session);
            let _ = app.emit("palette-text-changed", ());
        }
        Ok(Ok(false)) => {}
        Ok(Err(e)) => log::warn!("indexing the sent prompt: {e:#}"),
        Err(e) => log::warn!("indexing the sent prompt stopped: {e}"),
    }

    index.changed();
}

pub(super) async fn follow(app: &AppHandle) {
    let index = app.state::<Arc<PromptIndex>>().inner().clone();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let mut pacer = Pacer::default();
    let mut wake = None;

    loop {
        pacer.next_pass(&index.changed, wake).await;

        let (due, next) = index.due(app.state::<PaletteCache>().sessions());
        wake = next;
        let mut work = stream::iter(due)
            .map(|session| {
                let index = index.clone();
                let fleet = fleet.clone();
                async move {
                    let (id, activity) = (identity(&session), session.last_activity);
                    let saved = AtomicBool::new(false);
                    let pass = match timeout(SESSION_WAIT, refresh(&index, &fleet, session, &saved))
                        .await
                    {
                        Ok(Ok(pass)) => pass,
                        Ok(Err(e)) => {
                            log::debug!("indexing session prompts: {e:#}");
                            stopped(&saved)
                        }
                        Err(_) => stopped(&saved),
                    };
                    (id, activity, pass)
                }
            })
            .buffer_unordered(IN_FLIGHT);

        while let Some((id, activity, pass)) = work.next().await {
            match pass {
                Pass::Current | Pass::Indexed => index.schedule().indexed(id, activity),
                Pass::Partial => {
                    index.schedule().progressed(&id);
                    wake = sooner(wake, Instant::now());
                }
                Pass::Offline => {}
                Pass::Failed => {
                    let at = index.schedule().failed(id, Instant::now());
                    wake = sooner(wake, at);
                }
            }

            if matches!(pass, Pass::Indexed | Pass::Partial)
                && let Err(e) = app.emit("palette-text-changed", ())
            {
                log::warn!("telling the windows prompts changed: {e}");
            }
        }
    }
}

fn sooner(wake: Option<Instant>, at: Instant) -> Option<Instant> {
    Some(wake.map_or(at, |wake| wake.min(at)))
}

/// A read that stopped keeps the pages it saved.
fn stopped(saved: &AtomicBool) -> Pass {
    if saved.load(Ordering::Relaxed) {
        Pass::Partial
    } else {
        Pass::Failed
    }
}

async fn refresh(
    index: &Arc<PromptIndex>,
    fleet: &Fleet,
    session: MachineSession,
    saved: &AtomicBool,
) -> anyhow::Result<Pass> {
    let (id, stamp) = (identity(&session), stamp(&session)?);
    let machine = session.machine_id.parse()?;

    if !fleet.online().contains(&machine) {
        return Ok(Pass::Offline);
    }

    let store = index.clone();

    if spawn_blocking(move || store.store().current(&id, &stamp)).await?? {
        return Ok(Pass::Current);
    }

    let client = fleet.client(machine).await?;
    index_new(&client, index, &session, saved).await?;
    Ok(Pass::Indexed)
}

/// Reads the entries of `session` added since it was last indexed, newest
/// page first, and saves each page as it arrives.
async fn index_new<S: Sessions + ?Sized>(
    client: &S,
    index: &Arc<PromptIndex>,
    session: &MachineSession,
    saved: &AtomicBool,
) -> anyhow::Result<()> {
    let store = index.clone();
    let id = identity(session);
    let mut progress = spawn_blocking(move || store.store().progress(&id)).await??;
    // A walk left by an earlier read finishes first; the newest page is
    // read after it.
    let mut newest = false;

    loop {
        let before = progress.walk.map(|walk| walk.low);
        let (texts, page) = read_page(client, &session.id, before).await?;
        newest |= before.is_none();
        progress = advance(progress, page);
        let current = newest && progress.walk.is_none();

        let store = index.clone();
        let session = session.clone();
        spawn_blocking(move || store.store().save(&session, &texts, progress, current)).await??;
        saved.store(true, Ordering::Relaxed);

        if current {
            return Ok(());
        }
    }
}

/// Where a transcript page starts and how many entries it has. Thoughts
/// are left out of the entries but counted in `earlier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Page {
    earlier: u64,
    entries: u64,
}

/// The user's prompts on the page before entry `before` (the newest page
/// when `None`).
async fn read_page<S: Sessions + ?Sized>(
    client: &S,
    session: &str,
    before: Option<u64>,
) -> anyhow::Result<(Vec<String>, Page)> {
    let request = SessionsRequest::Transcript {
        session: session.to_owned(),
        last: PAGE,
        thoughts: false,
        before,
    };
    let (entries, earlier) = match client
        .sessions(request, Reading::Once, Duration::from_secs(10))
        .await?
    {
        SessionsReply::Transcript { entries, earlier } => (entries, earlier),
        SessionsReply::Error { message } => bail!("{message}"),
        _ => bail!("the machine did not return a transcript"),
    };
    let page = Page {
        earlier: u64::from(earlier),
        entries: entries.len() as u64,
    };
    let prompts = entries
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::User)
        .map(|entry| own_words(&entry.text))
        .filter(|text| !text.trim().is_empty())
        .collect();
    Ok((prompts, page))
}

/// How far the session is indexed once `page` is.
fn advance(progress: Progress, page: Page) -> Progress {
    let Some(walk) = progress.walk else {
        // The newest page. Thoughts are not counted in its entries, so
        // where it ends is a lower bound and at worst a page is read twice.
        let end = page.earlier + page.entries;

        return match progress.done {
            // The full reader of schema 1 read everything before.
            None => Progress {
                done: Some(end),
                walk: None,
            },
            Some(done) if page.earlier <= done => Progress {
                done: Some(done.max(end)),
                walk: None,
            },
            Some(done) => Progress {
                done: Some(done),
                walk: Some(Walk {
                    top: end.max(done),
                    low: page.earlier,
                }),
            },
        };
    };

    // A node from before `before` answers with its newest page again, and
    // past entries a session dropped the answer is empty: either way
    // nothing older can be read.
    let reached = !valid_page(walk.low, page) || page.earlier <= progress.done.unwrap_or(0);

    if reached {
        return Progress {
            done: Some(walk.top),
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

/// Prompt matches from disk, even when the machine holding the log is offline.
#[tauri::command]
pub async fn center_find_prompts(
    app: AppHandle,
    index: State<'_, Arc<PromptIndex>>,
    query: String,
) -> Result<Vec<Entry>> {
    let projects = app.state::<PaletteCache>().projects();
    let fleet = app.state::<Arc<Fleet>>();
    let members: Option<Vec<_>> = fleet.mesh().map(|mesh| {
        mesh.machines()
            .into_iter()
            .map(|d| d.device.id.to_string())
            .collect()
    });
    let index = index.inner().clone();
    Ok(spawn_blocking(move || -> anyhow::Result<Vec<Entry>> {
        index
            .reader()
            .find(&query, members.as_deref())?
            .into_iter()
            .map(|found| {
                let mut entry = session_entry(&found.session, &projects);
                let mut snippets = Vec::new();
                for term in query.split_whitespace() {
                    if let Some(text) = found
                        .texts
                        .iter()
                        .find(|text| text.to_lowercase().contains(&term.to_lowercase()))
                    {
                        let snippet = excerpt(text, term);
                        if !snippets.contains(&snippet) {
                            snippets.push(snippet);
                        }
                    }
                }
                entry.snippet = Some(format!("Prompt: {}", snippets.join(" · ")));
                Ok(entry)
            })
            .collect()
    })
    .await
    .context("searching session prompts stopped")??)
}

/// A short plain-text excerpt around the first match, with Unicode offsets
/// measured in characters so case folding cannot split a UTF-8 character.
fn excerpt(text: &str, query: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let query = query.trim().to_lowercase();
    let folded = text.to_lowercase();
    let folded_at = folded.find(&query).unwrap_or_default();
    let mut bytes = 0;
    let at = chars
        .iter()
        .take_while(|ch| {
            let before = bytes;
            bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
            before < folded_at
        })
        .count();
    let start = at.saturating_sub(48);
    let end = (at + query.chars().count() + 96).min(chars.len());
    let excerpt: String = chars[start.min(chars.len())..end].iter().collect();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        excerpt.split_whitespace().collect::<Vec<_>>().join(" "),
        if end < chars.len() { "…" } else { "" }
    )
}

#[cfg(test)]
#[path = "../tests/palette_prompts.rs"]
mod tests;
