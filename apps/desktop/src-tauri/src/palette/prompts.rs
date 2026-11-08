//! Background prompt indexing over the existing transcript protocol. The
//! menu reads SQLite, never a machine; unchanged sessions need no reread.

mod store;

use super::{index::Entry, lists::PaletteCache, places::Place, projects::session_entry};
use crate::{beep::AsyncMutex, error::Result, fleet::Fleet};
use anyhow::{Context, bail};
use beep_mesh::machine::sessions::Sessions;
use beep_node::agents::tags::own_words;
use beep_proto::{
    ImportedProject, MachineSession,
    node::agent::{EntryKind, SessionsReply, SessionsRequest},
};
use futures::{StreamExt, stream};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use store::{Store, identity, stamp};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{
    sync::Notify,
    task::spawn_blocking,
    time::{sleep, timeout},
};

const PAGE: u32 = 200;
const IN_FLIGHT: usize = 4;
const SESSION_WAIT: Duration = Duration::from_secs(60);
const RETRY_GAP: Duration = Duration::from_secs(30);

pub(crate) struct PromptIndex {
    store: Mutex<Store>,
    // WAL readers keep answering while a large session is being replaced.
    reader: Option<Mutex<Store>>,
    changed: Notify,
    project_changes: AsyncMutex<()>,
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
        })
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
        spawn_blocking(move || index.store().sync_projects(&projects)).await?
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
        app.state::<PaletteCache>()
            .forget_project(app, project, removed);
        let _ = app.emit("palette-text-changed", ());
        Ok(())
    }

    pub(super) fn changed(&self) {
        self.changed.notify_one();
    }

    pub(crate) async fn remove(self: &Arc<Self>, id: String) {
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

/// Called only after the node or server accepted the message. Indexing
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

pub(crate) async fn sent_existing(app: &AppHandle, machine: &str, session: &str, text: &str) {
    let known = app
        .state::<PaletteCache>()
        .sessions()
        .into_iter()
        .find(|s| s.machine_id == machine && s.id == session);
    let known = match known {
        Some(known) => Some(known),
        None => {
            let index = app.state::<Arc<PromptIndex>>().inner().clone();
            let id = format!("{machine} {session}");
            match spawn_blocking(move || index.reader().session(&id)).await {
                Ok(Ok(known)) => known,
                _ => None,
            }
        }
    };
    if let Some(known) = known {
        sent(app, known, text).await;
    }
}

pub(super) async fn follow(app: &AppHandle) {
    let index = app.state::<Arc<PromptIndex>>().inner().clone();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    loop {
        let sessions = app.state::<PaletteCache>().sessions();
        let mut work = stream::iter(sessions)
            .map(|session| {
                let index = index.clone();
                let fleet = fleet.clone();
                async move {
                    match timeout(SESSION_WAIT, refresh(&index, &fleet, session)).await {
                        Ok(Ok(changed)) => changed,
                        Ok(Err(e)) => {
                            log::debug!("indexing session prompts: {e:#}");
                            false
                        }
                        Err(_) => false,
                    }
                }
            })
            .buffer_unordered(IN_FLIGHT);
        while let Some(changed) = work.next().await {
            if changed && let Err(e) = app.emit("palette-text-changed", ()) {
                log::warn!("telling the windows prompts changed: {e}");
            }
        }
        // Failed or offline reads keep their old index and are retried.
        tokio::select! {
            () = sleep(RETRY_GAP) => {}
            () = index.changed.notified() => {}
        }
    }
}

async fn refresh(
    index: &Arc<PromptIndex>,
    fleet: &Fleet,
    session: MachineSession,
) -> anyhow::Result<bool> {
    let machine = session.machine_id.parse()?;
    if !fleet.online().contains(&machine) {
        return Ok(false);
    }
    let (id, stamp) = (identity(&session), stamp(&session)?);
    let saved = index.clone();
    if spawn_blocking(move || saved.store().current(&id, &stamp)).await?? {
        return Ok(false);
    }
    let client = fleet.client(machine).await?;
    let read = read_prompts(&client, &session.id).await?;
    let saved = index.clone();
    spawn_blocking(move || saved.store().replace(&session, &read.texts, read.complete)).await??;
    Ok(true)
}

struct ReadPrompts {
    texts: Vec<String>,
    complete: bool,
}

async fn read_prompts<S: Sessions + ?Sized>(
    client: &S,
    session: &str,
) -> anyhow::Result<ReadPrompts> {
    let mut before = None;
    let mut prompts = Vec::new();
    let mut complete = false;
    loop {
        // Include thoughts in the count so the pagination cursor has the
        // same meaning on every page, then retain only the user's words.
        let request = SessionsRequest::Transcript {
            session: session.to_owned(),
            last: PAGE,
            thoughts: true,
            before,
        };
        let (entries, earlier) = match client.sessions(request, Duration::from_secs(10)).await? {
            SessionsReply::Transcript { entries, earlier } => (entries, earlier),
            SessionsReply::Error { message } => bail!("{message}"),
            _ => bail!("the machine did not return a transcript"),
        };
        let earlier = u64::from(earlier);
        if !valid_page(before, earlier, entries.len()) {
            // An older node ignores `before`. Keep the usable latest
            // page instead of repeatedly appending it forever.
            break;
        }
        prompts.extend(
            entries
                .into_iter()
                .filter(|entry| entry.kind == EntryKind::User)
                .map(|entry| own_words(&entry.text))
                .filter(|text| !text.trim().is_empty()),
        );
        if earlier == 0 {
            complete = true;
            break;
        }
        before = Some(earlier);
    }
    Ok(ReadPrompts {
        texts: prompts,
        complete,
    })
}

fn valid_page(before: Option<u64>, earlier: u64, count: usize) -> bool {
    before
        .is_none_or(|before| earlier < before && earlier.checked_add(count as u64) == Some(before))
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
                entry.snippet = Some(format!("Prompt: {}", excerpt(&found.text, &query)));
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
mod tests {
    use super::*;

    struct Node {
        pages: Mutex<std::collections::VecDeque<SessionsReply>>,
        cursors: Mutex<Vec<Option<u64>>>,
    }

    impl Sessions for Node {
        async fn sessions(
            &self,
            request: SessionsRequest,
            _: Duration,
        ) -> std::result::Result<SessionsReply, beep_mesh::MeshError> {
            let SessionsRequest::Transcript {
                before, thoughts, ..
            } = request
            else {
                panic!("expected a transcript request")
            };
            assert!(thoughts, "pagination counts every entry");
            self.cursors.lock().expect("test lock").push(before);
            Ok(self
                .pages
                .lock()
                .expect("test lock")
                .pop_front()
                .expect("one request per page"))
        }
    }

    #[tokio::test]
    async fn reads_earlier_prompts_and_excludes_context_and_agent_output() -> anyhow::Result<()> {
        use beep_proto::node::agent::Entry;
        let node = Node {
            pages: Mutex::new(
                [
                    SessionsReply::Transcript {
                        earlier: 2,
                        entries: vec![
                            Entry::new(
                                EntryKind::User,
                                "<beep-context>hidden instructions</beep-context>add retries"
                                    .into(),
                            ),
                            Entry::new(EntryKind::Agent, "assistant output".into()),
                        ],
                    },
                    SessionsReply::Transcript {
                        earlier: 0,
                        entries: vec![
                            Entry::new(EntryKind::User, "original request".into()),
                            Entry::new(EntryKind::Thought, "reasoning".into()),
                        ],
                    },
                ]
                .into(),
            ),
            cursors: Mutex::default(),
        };
        let read = read_prompts(&node, "a").await?;
        assert_eq!(read.texts, ["add retries", "original request"]);
        assert!(read.complete);
        assert_eq!(*node.cursors.lock().expect("test lock"), [None, Some(2)]);
        Ok(())
    }

    #[test]
    fn a_broken_index_does_not_stop_the_app() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("search.sqlite");
        std::fs::write(&path, b"broken cache")?;
        let index = PromptIndex::open(&path)?;
        assert!(index.reader().find("retry", None)?.is_empty());
        Ok(())
    }

    #[test]
    fn old_nodes_and_dropped_transcripts_cannot_repeat_a_page() {
        assert!(valid_page(None, 400, 200));
        assert!(valid_page(Some(400), 200, 200));
        assert!(!valid_page(Some(400), 400, 200));
        assert!(!valid_page(Some(400), 250, 200));
        assert!(!valid_page(Some(400), 400, 0));
    }

    #[test]
    fn excerpt_includes_a_late_match() {
        let text = format!(
            "{} Add a retry button. {}",
            "earlier ".repeat(200),
            "later ".repeat(200)
        );
        let found = excerpt(&text, "RETRY");
        assert!(found.contains("retry button"));
        assert!(found.starts_with('…') && found.ends_with('…'));
        assert!(found.len() < 180);
        assert_eq!(excerpt("修正 接続を再試行", "再試行"), "修正 接続を再試行");
    }
}
