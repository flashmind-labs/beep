//! The bell's history on this Mac: what the mesh told the user, kept in one
//! file (the newest `KEEP`, none older than `MAX_AGE_MS`), so the pane lists
//! it with no server. Coding agents' notices come from `route`; sync
//! conflicts, failed moves and machines that went away from `watch`.

use super::notify;
use crate::{
    beep::epoch_ms,
    center::{AgentTarget, CenterTarget},
    error::{DesktopError, Result},
    tray::waiting,
};
use anyhow::Context;
use beep_proto::{AgentNoticeKey, ApiError, Notice};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::SystemTime,
};
use tauri::{AppHandle, Emitter, Manager, State};

/// The most notes kept, newest first.
const KEEP: usize = 200;
/// How long a note is kept.
const MAX_AGE_MS: u64 = 30 * 24 * 3600 * 1000;

/// What a note is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    /// A coding agent's turn ended.
    Finished,
    /// A coding agent asks the user; `done` once answered.
    Asks,
    /// Sync left conflict copies on a machine; `done` once settled.
    Conflict,
    /// A session move failed.
    MoveFailed,
    /// A machine went offline; `done` once it is back.
    Offline,
    /// A skill has other files on another machine or agent; `done` once alike.
    SkillConflict,
}

/// One row of the bell's pane from the mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Names what it is about: a newer note with the same id replaces it.
    pub(crate) id: String,
    pub(crate) kind: NoteKind,
    pub(crate) title: String,
    pub(crate) text: String,
    /// The machine or project it comes from.
    pub(crate) source: String,
    /// Milliseconds since the Unix epoch.
    pub(crate) at: u64,
    pub(crate) read: bool,
    /// Answered, settled or back online.
    pub(crate) done: bool,
    /// The session a click opens, for agents and moves.
    pub(crate) target: Option<CenterTarget>,
}

impl Note {
    /// An unread note made now, with no text and nothing to open.
    pub(crate) fn new(kind: NoteKind, id: String, title: String, source: String) -> Self {
        Self {
            id,
            kind,
            title,
            text: String::new(),
            source,
            at: now_ms(),
            read: false,
            done: false,
            target: None,
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    epoch_ms(SystemTime::now())
}

/// The notes and the file they are kept in.
pub struct Notes {
    path: PathBuf,
    list: Mutex<Vec<Note>>,
    /// One write at a time, each of the newest list.
    saving: Mutex<()>,
}

impl Notes {
    /// Reads the file at `path`; a missing or unreadable one starts empty.
    pub(crate) fn load(path: PathBuf) -> Self {
        let list: Vec<Note> = fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?))
            .unwrap_or_else(|e| {
                log::info!("starting without the saved notifications: {e:#}");
                Vec::new()
            });
        Self {
            path,
            list: Mutex::new(list),
            saving: Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Note>> {
        self.list.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every note, newest first.
    pub(crate) fn list(&self) -> Vec<Note> {
        self.lock().clone()
    }

    pub(crate) fn get(&self, id: &str) -> Option<Note> {
        self.lock().iter().find(|note| note.id == id).cloned()
    }

    /// Takes `note` in place of any with its id, and drops what is past
    /// `KEEP` or older than `MAX_AGE_MS` at `now`.
    fn put(&self, note: Note, now: u64) {
        let mut list = self.lock();
        list.retain(|old| old.id != note.id);
        list.push(note);
        list.sort_by_key(|note| Reverse(note.at));
        list.retain(|note| now.saturating_sub(note.at) <= MAX_AGE_MS);
        list.truncate(KEEP);
    }

    /// Runs `change` on every note; true when it changed one.
    fn change(&self, mut change: impl FnMut(&mut Note) -> bool) -> bool {
        let mut changed = false;
        for note in self.lock().iter_mut() {
            changed |= change(note);
        }
        changed
    }

    /// Marks done every note `which` picks that is not yet.
    fn finish(&self, which: impl Fn(&Note) -> bool) -> bool {
        self.change(|note| {
            let now = !note.done && which(note);
            note.done |= now;
            now
        })
    }

    fn read(&self, ids: &[String]) -> bool {
        self.change(|note| {
            let now = !note.read && ids.contains(&note.id);
            note.read |= now;
            now
        })
    }

    fn dismiss(&self, ids: &[String]) -> bool {
        let mut list = self.lock();
        let before = list.len();
        list.retain(|note| !ids.contains(&note.id));
        list.len() != before
    }

    /// Writes the newest list, through a partial file so a reader never sees
    /// half of it. It blocks.
    fn save(&self) -> anyhow::Result<()> {
        let _one = self.saving.lock().unwrap_or_else(PoisonError::into_inner);
        let bytes = serde_json::to_vec(&*self.lock())?;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        }
        let mut partial = self.path.as_os_str().to_owned();
        partial.push(".part");
        fs::write(&partial, &bytes).with_context(|| format!("writing {}", self.path.display()))?;
        fs::rename(&partial, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))?;
        Ok(())
    }
}

fn notes(app: &AppHandle) -> Arc<Notes> {
    app.state::<Arc<Notes>>().inner().clone()
}

/// Tells the windows and writes the file.
fn changed(app: &AppHandle, notes: Arc<Notes>) {
    if let Err(e) = app.emit("notes", notes.list()) {
        log::warn!("telling the windows about the notifications: {e}");
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = notes.save() {
            log::warn!("saving the notifications: {e:#}");
        }
    });
}

/// Adds `note`, in place of any with its id.
pub(crate) fn record(app: &AppHandle, note: Note) {
    let notes = notes(app);
    notes.put(note, now_ms());
    changed(app, notes);
}

/// Marks done the notes `which` picks.
pub(crate) fn finish(app: &AppHandle, which: impl Fn(&Note) -> bool) {
    let notes = notes(app);
    if notes.finish(which) {
        changed(app, notes);
    }
}

/// The note with `id`, if it is kept.
pub(crate) fn get(app: &AppHandle, id: &str) -> Option<Note> {
    notes(app).get(id)
}

/// The notes of `kind` not done yet.
pub(crate) fn open_of(app: &AppHandle, kind: NoteKind) -> Vec<Note> {
    let mut list = notes(app).list();
    list.retain(|note| note.kind == kind && !note.done);
    list
}

/// The agent question an ask note is about.
fn asked(note: &Note) -> Option<AgentNoticeKey> {
    if note.kind != NoteKind::Asks {
        return None;
    }
    note.id.parse().ok()
}

/// A coding agent's notice: a question, or a turn that ended. Anything the
/// session asked before no longer waits. `seen` when it was on screen.
pub(crate) fn agent_notice(app: &AppHandle, key: &AgentNoticeKey, notice: &Notice, seen: bool) {
    let notes = notes(app);
    notes.finish(|note| {
        asked(note).is_some_and(|asked| {
            asked.is_session(&key.machine_id, &key.agent, &key.session)
                && asked.question != key.question
        })
    });
    let kind = if key.question.is_some() {
        NoteKind::Asks
    } else {
        NoteKind::Finished
    };
    let note = Note {
        text: notice.body.clone(),
        read: seen,
        target: Some(CenterTarget::Agent(AgentTarget::notified(key))),
        ..Note::new(
            kind,
            key.to_string(),
            notice.title.clone(),
            key.machine_name.clone(),
        )
    };
    notes.put(note, now_ms());
    changed(app, notes);
}

/// The question `key` names was answered.
pub(crate) fn answered(app: &AppHandle, key: &AgentNoticeKey) {
    finish(app, |note| {
        asked(note).is_some_and(|asked| {
            asked.is_session(&key.machine_id, &key.agent, &key.session)
                && asked.question == key.question
        })
    });
}

/// What the tray listed as waiting and no longer does, by its menu ids:
/// those questions were answered, here or anywhere.
pub(crate) fn no_longer_waiting(app: &AppHandle, gone: &[String]) {
    if gone.is_empty() {
        return;
    }
    finish(app, |note| {
        asked(note).is_some_and(|key| {
            key.question.is_some_and(|question| {
                gone.contains(&waiting::agent_item(
                    &key.machine_id,
                    &key.session,
                    question,
                ))
            })
        })
    });
}

/// The notes, newest first.
#[tauri::command]
pub fn notes_list(notes: State<'_, Arc<Notes>>) -> Vec<Note> {
    notes.list()
}

#[tauri::command]
pub fn notes_read(app: AppHandle, notes: State<'_, Arc<Notes>>, ids: Vec<String>) {
    if notes.read(&ids) {
        changed(&app, notes.inner().clone());
    }
}

#[tauri::command]
pub fn notes_dismiss(app: AppHandle, notes: State<'_, Arc<Notes>>, ids: Vec<String>) {
    if notes.dismiss(&ids) {
        changed(&app, notes.inner().clone());
    }
}

/// Answers the question ask note `id` is about, as Allow and Deny on its
/// macOS notification do. One answered elsewhere counts as answered.
#[tauri::command]
pub async fn notes_answer(app: AppHandle, id: String, allow: bool) -> Result<()> {
    let key: AgentNoticeKey = id
        .parse()
        .map_err(|_| DesktopError::BadRequest("not a question".into()))?;
    match notify::answer_agent(&app, key.clone(), allow).await {
        Ok(()) | Err(DesktopError::Api(ApiError::Conflict { .. } | ApiError::NotFound {})) => {
            answered(&app, &key);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const DAY_MS: u64 = 24 * 3600 * 1000;

    fn note(id: &str, at: u64) -> Note {
        Note {
            at,
            ..Note::new(
                NoteKind::Finished,
                id.into(),
                format!("Title {id}"),
                "build-box".into(),
            )
        }
    }

    fn ids(notes: &Notes) -> Vec<String> {
        notes.list().into_iter().map(|note| note.id).collect()
    }

    fn store(dir: &TempDir) -> Notes {
        Notes::load(dir.path().join("notes.json"))
    }

    #[test]
    fn notes_are_newest_first_and_a_newer_one_replaces_its_id() {
        let dir = TempDir::new().expect("a temp dir");
        let notes = store(&dir);
        let now = 100 * DAY_MS;
        notes.put(note("a", now - 3), now);
        notes.put(note("b", now - 1), now);
        // An offline note is dated when the machine went away, before the others.
        notes.put(note("c", now - 5), now);
        assert_eq!(ids(&notes), ["b", "a", "c"]);
        notes.put(note("a", now), now);
        assert_eq!(ids(&notes), ["a", "b", "c"]);
    }

    #[test]
    fn only_the_newest_and_the_last_thirty_days_are_kept() {
        let dir = TempDir::new().expect("a temp dir");
        let notes = store(&dir);
        let now = 100 * DAY_MS;
        notes.put(note("old", now - 31 * DAY_MS), now);
        notes.put(note("month", now - 30 * DAY_MS), now);
        assert_eq!(ids(&notes), ["month"]);
        for i in 0..KEEP as u64 + 5 {
            notes.put(note(&format!("n{i}"), now - 1000 + i), now);
        }
        let kept = ids(&notes);
        assert_eq!(kept.len(), KEEP);
        assert_eq!(kept[0], format!("n{}", KEEP + 4));
        assert!(!kept.contains(&"month".to_owned()));
    }

    #[test]
    fn read_finish_and_dismiss_say_when_they_changed_something() {
        let dir = TempDir::new().expect("a temp dir");
        let notes = store(&dir);
        notes.put(note("a", 1), 1);
        notes.put(note("b", 2), 2);
        assert!(notes.read(&["a".into()]));
        assert!(!notes.read(&["a".into(), "gone".into()]));
        assert!(notes.get("a").is_some_and(|n| n.read));
        assert!(notes.get("b").is_some_and(|n| !n.read));
        assert!(notes.finish(|n| n.id == "b"));
        assert!(!notes.finish(|n| n.id == "b"));
        assert!(notes.dismiss(&["a".into()]));
        assert!(!notes.dismiss(&["a".into()]));
        assert_eq!(ids(&notes), ["b"]);
    }

    #[test]
    fn notes_outlive_the_app() {
        let dir = TempDir::new().expect("a temp dir");
        let notes = store(&dir);
        let mut asks = note("a", 7);
        asks.kind = NoteKind::Asks;
        asks.target = Some(CenterTarget::Home);
        notes.put(asks.clone(), 7);
        notes.save().expect("the notes are written");
        assert_eq!(store(&dir).list(), [asks]);
        // A file that does not read starts empty.
        fs::write(dir.path().join("notes.json"), "{").expect("written");
        assert!(store(&dir).list().is_empty());
    }

    #[test]
    fn an_ask_names_its_question_and_its_tray_item() {
        let key = AgentNoticeKey {
            machine_id: "m1".into(),
            agent: "claude".into(),
            session: "s1".into(),
            question: Some(3),
            machine_name: "build-box".into(),
        };
        let mut asks = note(&key.to_string(), 1);
        assert_eq!(asked(&asks), None, "a finished turn asks nothing");
        asks.kind = NoteKind::Asks;
        assert_eq!(asked(&asks), Some(key));
        assert_eq!(waiting::agent_item("m1", "s1", 3), "wait:agent:m1:s1:3");
    }
}
