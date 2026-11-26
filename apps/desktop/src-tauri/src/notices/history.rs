//! The bell's history on this Mac: what the mesh told the user, kept in one
//! file (the newest `KEEP`, none older than `MAX_AGE_MS`), so the pane lists
//! it at once. Coding agents' notices and CI runs come from `route`;
//! sync conflicts, failed moves and machines that went away from `watch`.

use crate::{
    beep::epoch_ms,
    center::{AgentTarget, CenterTarget, RunAt},
    chat::markdown::inline_html,
    error::{DesktopError, Result},
    fleet::Fleet,
    store::{lock, write_part},
};
use beep_fleet::{Read, digest};
use beep_proto::SessionId;
use beep_proto::{
    AgentNoticeKey, Notice, RunState,
    node::{
        agent::RunNews,
        skills::{self, Held},
    },
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::HashSet,
    fs,
    path::{Component, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::SystemTime,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::broadcast::error::RecvError;

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
    /// A coding agent asks the user; removed once answered.
    Asks,
    /// Sync left conflict copies on a machine; `done` once settled.
    Conflict,
    /// A session move failed.
    MoveFailed,
    /// A machine went offline; `done` once it is back.
    Offline,
    /// A skill has other files on another machine or agent; `done` once alike.
    SkillConflict,
    /// A CI run the user started ended.
    Run,
    /// A temporary machine, such as a container, was offline so long that this Mac removed it.
    Removed,
}

/// One row of the bell's pane from the mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Names what it is about: a newer note with the same id replaces it.
    pub(crate) id: String,
    pub(crate) kind: NoteKind,
    pub(crate) title: String,
    pub(crate) text: String,
    /// `text` as inline HTML, for a coding agent's reply in markdown.
    #[serde(default)]
    pub(crate) html: String,
    /// The machine or project it comes from.
    pub(crate) source: String,
    /// Milliseconds since the Unix epoch.
    pub(crate) at: u64,
    pub(crate) read: bool,
    /// Answered, settled or back online.
    pub(crate) done: bool,
    /// The session a click opens, for agents and moves.
    pub(crate) target: Option<CenterTarget>,
    /// The lines of `text` that are on this Mac, for a skill conflict.
    #[serde(default)]
    pub(crate) here: Vec<String>,
    /// Each copy of a skill conflict, to keep one.
    #[serde(default)]
    pub(crate) held: Vec<Held>,
    /// Allow and Deny answer its question; any other question opens its chat.
    #[serde(default)]
    pub(crate) allow_deny: bool,
}

impl Note {
    /// What names this version of the note, the same on every app.
    pub(crate) fn digest(&self) -> u64 {
        digest(&[&format!("{:?}", self.kind), &self.title, &self.text])
    }

    /// What an app marks when it reads this version of the note.
    pub(crate) fn mark(&self) -> Read {
        (self.id.clone(), self.digest())
    }

    /// An unread note made now, with no text and nothing to open.
    pub(crate) fn new(kind: NoteKind, id: String, title: String, source: String) -> Self {
        Self {
            id,
            kind,
            title,
            text: String::new(),
            html: String::new(),
            source,
            at: now_ms(),
            read: false,
            done: false,
            target: None,
            here: Vec::new(),
            held: Vec::new(),
            allow_deny: false,
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    epoch_ms(SystemTime::now())
}

/// Whether an app read this version of `note`. A later turn under the same id says something
/// else, so it is unread.
fn read_since(marks: &HashSet<Read>, note: &Note) -> bool {
    marks.contains(&note.mark())
}

/// The reads among `notes`, those not read yet.
fn unread_of(notes: Vec<Note>) -> impl Iterator<Item = Read> {
    notes
        .into_iter()
        .filter(|note| !note.read)
        .map(|note| note.mark())
}

/// What a coding agent's notice did to the notes.
struct Noted {
    changed: bool,
    /// What it marked read, for the other apps.
    read: Vec<Read>,
    /// The note it left unread, by id and digest, which may show as a notification.
    unread: Option<Read>,
}

/// The notes and the file they are kept in.
pub struct Notes {
    path: PathBuf,
    list: Mutex<Vec<Note>>,
    /// The notes read on any app of the mesh, by id and digest. Locked before `list`.
    marks: Mutex<HashSet<Read>>,
    /// One write at a time, each of the newest list.
    saving: Mutex<()>,
}

impl Notes {
    /// Reads the file at `path`; a missing or unreadable one starts empty.
    pub(crate) fn load(path: PathBuf) -> Self {
        let mut list: Vec<Note> = fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?))
            .unwrap_or_else(|e| {
                log::info!("starting without the saved notifications: {e:#}");
                Vec::new()
            });
        list.retain(|note| note.kind != NoteKind::Asks || !note.done);

        Self {
            path,
            list: Mutex::new(list),
            marks: Mutex::new(HashSet::new()),
            saving: Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Note>> {
        lock(&self.list)
    }

    fn lock_marks(&self) -> MutexGuard<'_, HashSet<Read>> {
        lock(&self.marks)
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
    fn put(&self, mut note: Note, now: u64) {
        let marks = self.lock_marks();
        note.read |= read_since(&marks, &note);
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

    /// Removes answered questions and marks other matching notes done.
    fn finish(&self, which: impl Fn(&Note) -> bool) -> bool {
        let mut changed = false;
        self.lock().retain_mut(|note| {
            if note.done || !which(note) {
                return true;
            }

            changed = true;
            note.done = true;
            note.kind != NoteKind::Asks
        });

        changed
    }

    /// Marks read the notes in `ids` that are not yet; returns what was read.
    fn read(&self, ids: &[String]) -> Vec<Read> {
        let mut read = Vec::new();
        self.change(|note| {
            let now = !note.read && ids.contains(&note.id);
            note.read |= now;
            if now {
                read.push(note.mark());
            }
            now
        });
        read
    }

    /// Takes the reads the apps of the mesh share; true when a note became read.
    fn shared_reads(&self, marks: HashSet<Read>) -> bool {
        let mut held = self.lock_marks();
        *held = marks;
        self.change(|note| {
            let now = !note.read && read_since(&held, note);
            note.read |= now;
            now
        })
    }

    /// Whether the note `id` is kept and unread in the version `digest` names.
    fn unread(&self, id: &str, digest: u64) -> bool {
        self.lock()
            .iter()
            .any(|note| note.id == id && !note.read && note.digest() == digest)
    }

    /// Removes the notes in `ids`; returns them.
    fn dismiss(&self, ids: &[String]) -> Vec<Note> {
        self.remove(|note| ids.contains(&note.id))
    }

    /// Opening a session removes its finished-turn notes, including read ones; returns them.
    fn viewed_agent(&self, machine: &str, agent: &str, session: &SessionId) -> Vec<Note> {
        self.remove(|note| {
            note.kind == NoteKind::Finished
                && note.id.parse::<AgentNoticeKey>().is_ok_and(|key| {
                    key.question.is_none() && key.is_session(machine, agent, session)
                })
        })
    }

    fn remove(&self, which: impl Fn(&Note) -> bool) -> Vec<Note> {
        let mut list = self.lock();
        let (gone, kept) = list.drain(..).partition(|note| which(note));
        *list = kept;
        gone
    }

    /// Keeps questions, but a finished turn on screen needs no note.
    fn agent_notice(
        &self,
        key: &AgentNoticeKey,
        notice: &Notice,
        allow_deny: bool,
        seen: bool,
        reviewer: bool,
    ) -> Noted {
        let finished = self.finish(|note| {
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
        let target = AgentTarget::notified(key);
        let target = if reviewer { target.reviewing() } else { target };
        let note = Note {
            text: notice.body.clone(),
            html: inline_html(&notice.body),
            read: seen,
            target: Some(CenterTarget::Agent(target)),
            allow_deny: allow_deny && key.question.is_some(),
            ..Note::new(
                kind,
                key.to_string(),
                notice.title.clone(),
                key.machine_name.clone(),
            )
        };
        let this = note.mark();
        // Read on screen here, so it is read on every app.
        let mut read = if seen { vec![this.clone()] } else { Vec::new() };

        if seen && key.question.is_none() {
            let removed = self.viewed_agent(&key.machine_id, &key.agent, &key.session);
            let changed = finished || !removed.is_empty();
            read.extend(unread_of(removed));
            return Noted {
                changed,
                read,
                unread: None,
            };
        }

        self.put(note, now_ms());
        let unread = self.unread(&this.0, this.1).then_some(this);
        Noted {
            changed: true,
            read,
            unread,
        }
    }

    /// Writes the newest list, through a partial file so a reader never sees
    /// half of it. It blocks.
    fn save(&self) -> anyhow::Result<()> {
        let _one = lock(&self.saving);
        let bytes = serde_json::to_vec(&*self.lock())?;
        write_part(&self.path, &bytes)
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

/// Removes matching questions and marks other matching notes done.
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
pub(crate) fn asked(note: &Note) -> Option<AgentNoticeKey> {
    if note.kind != NoteKind::Asks {
        return None;
    }

    note.id.parse().ok()
}

/// A coding agent's notice: a question, or a turn that ended off screen.
/// Anything the session asked before no longer waits. Returns the note it
/// left unread, by id and digest: read on no app of the mesh yet.
pub(crate) fn agent_notice(
    app: &AppHandle,
    key: &AgentNoticeKey,
    notice: &Notice,
    allow_deny: bool,
    seen: bool,
    reviewer: bool,
) -> Option<Read> {
    let notes = notes(app);
    take_reads(app, &notes);
    let noted = notes.agent_notice(key, notice, allow_deny, seen, reviewer);

    if noted.changed {
        changed(app, notes);
    }

    share(app, noted.read);
    noted.unread
}

/// The user opened this session; its finished-turn notifications no longer
/// wait, here or on any other app.
pub(crate) fn agent_viewed(app: &AppHandle, machine: &str, agent: &str, session: &SessionId) {
    let notes = notes(app);
    let removed = notes.viewed_agent(machine, agent, session);

    if removed.is_empty() {
        return;
    }

    changed(app, notes);
    share(app, unread_of(removed).collect());
}

/// Whether note `read` names is still unread, after taking the reads the mesh holds now.
pub(crate) fn still_unread(app: &AppHandle, (id, digest): &Read) -> bool {
    let notes = notes(app);
    take_reads(app, &notes);
    notes.unread(id, *digest)
}

/// Takes the reads the mesh holds now, so a notice that comes before `follow_reads` looked does
/// not show a note another app read.
fn take_reads(app: &AppHandle, notes: &Arc<Notes>) {
    let Some(mesh) = app.state::<Arc<Fleet>>().mesh() else {
        return;
    };

    if notes.shared_reads(beep_fleet::read_marks(&mesh)) {
        changed(app, notes.clone());
    }
}

/// Writes `read` to this app's marks in the mesh, for every other app.
fn share(app: &AppHandle, read: Vec<Read>) {
    if read.is_empty() {
        return;
    }

    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let marked: Result<()> = async {
            let mesh = fleet.ready().await?;
            Ok(beep_fleet::mark_read(&mesh, &read, now_ms()).await?)
        }
        .await;

        if let Err(e) = marked {
            log::warn!("sharing notifications read here: {e}");
        }
    });
}

/// The note for a run a machine told of: "Build failed on main" from its
/// project. `None` for a run that was skipped, which says nothing.
fn run_note(news: &RunNews) -> Option<Note> {
    let run = &news.run;
    let ended = match run.state {
        RunState::Passed {} => "passed",
        RunState::Failed {} => "failed",
        RunState::Cancelled {} => "cancelled",
        _ => return None,
    };
    let title = if run.branch.is_empty() {
        format!("Build {ended}")
    } else {
        format!("Build {ended} on {}", run.branch)
    };
    let text = match run.number {
        0 => run.workflow.clone(),
        number => format!("{} #{number}", run.workflow),
    };
    Some(Note {
        text,
        target: Some(CenterTarget::Projects {
            project: Some(news.project.clone()),
            run: Some(RunAt {
                id: run.id,
                host: news.host.clone(),
            }),
        }),
        ..Note::new(
            NoteKind::Run,
            format!("run:{}:{}:{}", news.project, news.host, run.id),
            title,
            news.project.clone(),
        )
    })
}

/// A run a machine told of: the note it makes, `None` when there is none to
/// make or one was made already (another machine told it too). The note is
/// read when another app read it.
pub(crate) fn run_ended(app: &AppHandle, news: &RunNews) -> Option<Note> {
    let note = run_note(news)?;
    let notes = notes(app);

    if notes.get(&note.id).is_some() {
        return None;
    }

    take_reads(app, &notes);
    notes.put(note.clone(), now_ms());
    changed(app, notes.clone());
    notes.get(&note.id)
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

/// The notes, newest first.
#[tauri::command]
pub fn notes_list(notes: State<'_, Arc<Notes>>) -> Vec<Note> {
    notes.list()
}

/// Marks notes read here and, for every other app, in the mesh.
#[tauri::command]
pub async fn notes_read(
    app: AppHandle,
    notes: State<'_, Arc<Notes>>,
    fleet: State<'_, Arc<Fleet>>,
    ids: Vec<String>,
) -> Result<()> {
    let read = notes.read(&ids);

    if read.is_empty() {
        return Ok(());
    }

    changed(&app, notes.inner().clone());
    let mesh = fleet.ready().await?;
    beep_fleet::mark_read(&mesh, &read, now_ms()).await?;
    Ok(())
}

/// Takes the reads other apps make, as the mesh tells of them, for as long as the app runs.
pub(crate) async fn follow_reads(app: AppHandle, fleet: Arc<Fleet>) {
    let mesh = match fleet.ready().await {
        Ok(mesh) => mesh,
        Err(e) => {
            log::warn!("notifications read elsewhere are not followed: {e}");
            return;
        }
    };
    // Subscribed before the first scan, so no change falls between the two.
    let mut changes = mesh.changes();

    loop {
        let notes = notes(&app);

        if notes.shared_reads(beep_fleet::read_marks(&mesh)) {
            changed(&app, notes);
        }

        loop {
            match changes.recv().await {
                Ok(beep_mesh::Key::NoticeRead(_)) | Err(RecvError::Lagged(_)) => break,
                Ok(_) => {}
                Err(RecvError::Closed) => return,
            }
        }
    }
}

/// Removes notes here; on every other app they are read.
#[tauri::command]
pub fn notes_dismiss(app: AppHandle, notes: State<'_, Arc<Notes>>, ids: Vec<String>) {
    let removed = notes.dismiss(&ids);

    if removed.is_empty() {
        return;
    }

    changed(&app, notes.inner().clone());
    share(&app, unread_of(removed).collect());
}

/// Shows skill `name` of a skills `folder` on this Mac, such as
/// `~/.claude/skills`, in Finder.
#[tauri::command]
pub fn skill_reveal(app: AppHandle, folder: String, name: String) -> Result<()> {
    let home = app.path().home_dir()?;
    let skill = match folder.strip_prefix("~/") {
        Some(rest) => home.join(rest).join(&name),
        None => PathBuf::from(&folder).join(&name),
    };
    let climbs = skill.components().any(|part| part == Component::ParentDir);

    if name.contains(['/', '\\']) || climbs || !skill.starts_with(&home) || !skill.is_dir() {
        return Err(DesktopError::BadRequest(
            "not a skill on this computer".into(),
        ));
    }

    crate::platform::reveal(&skill)?;
    Ok(())
}

/// Keeps `machine`'s copy of skill `name` that hashes to `hash`: that node
/// makes it the picked version, and every other copy takes it.
#[tauri::command]
pub async fn skill_keep(
    fleet: State<'_, Arc<Fleet>>,
    machine: EndpointId,
    name: String,
    hash: String,
) -> Result<()> {
    fleet
        .client(machine)
        .await?
        .call::<_, skills::Reply>(skills::PROTOCOL, &skills::Request::Keep { name, hash })
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/notices_history.rs"]
mod tests;
