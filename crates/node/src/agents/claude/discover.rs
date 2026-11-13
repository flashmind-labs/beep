//! Finding Claude Code sessions on the machine (`Store`, `list`,
//! `sightings`): transcripts under `<config>/projects/`, and which of them a
//! running `claude` has open.

use super::{procs::Process, transcript};
use crate::agents::{
    found::{Agent, Memo, Seen as Sighting},
    place::of_folder,
};
use beep_proto::node::agent::{ExternalSession, Presence};
use serde::Deserialize;
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// A transcript written this recently counts as active even when no
/// process is found for it.
const RECENT: Duration = Duration::from_secs(3 * 60);
/// The most transcript files looked at in one listing.
const MAX_FILES: usize = 20_000;
/// The longest a pid file may be.
const MAX_PID_FILE: u64 = 64 * 1024;
/// The most project folders whose sessions `sightings` reads.
const MAX_FOLDERS: usize = 1_000;
/// The most transcript heads read in one folder for where it is.
const HEADS_PER_FOLDER: usize = 3;
/// The most transcript summaries kept.
const MAX_SUMMARIES: usize = 1_000;

/// Where Claude Code keeps its state, `~/.claude` unless
/// `CLAUDE_CONFIG_DIR` says otherwise.
#[derive(Debug, Clone)]
pub(crate) struct Store {
    root: PathBuf,
    memos: Arc<Memos>,
}

/// What transcripts said, kept while they are unchanged.
#[derive(Debug)]
struct Memos {
    /// The folder each transcript's head names.
    cwds: Memo<Option<String>>,
    summaries: Memo<Summary>,
    /// The newest transcript of each project folder, by its id and time,
    /// kept while the folder itself is unchanged: no file came or went.
    newest: Memo<Option<(String, SystemTime)>>,
}

impl Default for Memos {
    fn default() -> Self {
        Self {
            cwds: Memo::new(MAX_FOLDERS * HEADS_PER_FOLDER * 2),
            summaries: Memo::new(MAX_SUMMARIES),
            newest: Memo::new(MAX_FOLDERS),
        }
    }
}

/// What a transcript says, as a listing shows it.
#[derive(Debug, Clone)]
struct Summary {
    /// Its tail; its `last_at` is the Unix seconds of its last timestamped
    /// line. Claude Code also appends lines without one, such as its
    /// artifact ledger, between turns.
    gist: transcript::Gist,
    /// Unix seconds.
    started_at: u64,
}

/// One transcript file.
#[derive(Debug, Clone)]
pub(super) struct Transcript {
    pub(super) id: String,
    pub(super) path: PathBuf,
    pub(super) modified: SystemTime,
    created: Option<SystemTime>,
}

/// What Claude Code says about one of its processes, in
/// `<config>/sessions/<pid>.json`: the sure way to tie a process to a
/// session. Without one (older versions), a `claude` in a folder is taken
/// to run that folder's newest session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct PidFile {
    pub(super) pid: u32,
    #[serde(rename = "sessionId")]
    pub(super) session_id: String,
    /// Such as `busy` or `idle`.
    #[serde(default)]
    pub(super) status: Option<String>,
}

/// The machine as a listing sees it.
#[derive(Debug, Default, Clone)]
pub(super) struct Snapshot {
    pub(super) processes: Vec<Process>,
    pub(super) pid_files: Vec<PidFile>,
    /// Sessions this node runs: the agent's id to the node's.
    pub(super) owned: HashMap<String, String>,
    pub(super) now: Option<SystemTime>,
}

impl Snapshot {
    /// The machine now: its `processes`, the process files in `store`, and
    /// the sessions this node runs.
    pub(super) fn of(
        store: &Store,
        processes: Vec<Process>,
        owned: HashMap<String, String>,
    ) -> Self {
        Self {
            processes,
            pid_files: store.pid_files(),
            owned,
            now: None,
        }
    }
}

impl Store {
    /// The store on this machine, when Claude Code has one.
    pub(crate) fn locate() -> Option<Self> {
        Self::in_root(&super::config_dir()?)
    }

    /// The store in Claude Code's folder `root`, once it has `projects`.
    pub(super) fn in_root(root: &Path) -> Option<Self> {
        root.join("projects").is_dir().then(|| Self {
            root: root.to_owned(),
            memos: Arc::default(),
        })
    }

    /// The folder of the projects' transcripts.
    pub(super) fn projects(&self) -> PathBuf {
        self.root.join("projects")
    }

    #[cfg(test)]
    pub(super) fn at(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            memos: Arc::default(),
        }
    }

    /// Every top-level transcript, newest first.
    pub(super) fn transcripts(&self) -> io::Result<Vec<Transcript>> {
        let mut found = Vec::new();

        for project in fs::read_dir(self.projects())? {
            let project = project?.path();
            let Ok(files) = fs::read_dir(&project) else {
                continue;
            };

            for file in files.flatten() {
                if found.len() >= MAX_FILES {
                    break;
                }

                if let Some(transcript) = transcript_with(&file.path(), || file.metadata()) {
                    found.push(transcript);
                }
            }
        }

        found.sort_by_key(|transcript| Reverse(transcript.modified));
        Ok(found)
    }

    /// The transcript of session `id`.
    pub(super) fn find(&self, id: &str) -> Option<PathBuf> {
        if let Ok(crate::agents::subagent::Conversation::Claude { parent, agent }) =
            crate::agents::subagent::Conversation::try_from(id)
        {
            let path = self
                .find(parent)?
                .with_extension("")
                .join("subagents")
                .join(format!("agent-{agent}.jsonl"));
            return path.is_file().then_some(path);
        }

        if !is_session_id(id) {
            return None;
        }

        let name = format!("{id}.jsonl");
        fs::read_dir(self.projects())
            .ok()?
            .flatten()
            .map(|project| project.path().join(&name))
            .find(|path| path.is_file())
    }

    /// The processes Claude Code says it runs, live or not.
    pub(super) fn pid_files(&self) -> Vec<PidFile> {
        let Ok(files) = fs::read_dir(self.root.join("sessions")) else {
            return Vec::new();
        };
        files
            .flatten()
            .filter(|file| file.path().extension().is_some_and(|ext| ext == "json"))
            .filter(|file| file.metadata().is_ok_and(|meta| meta.len() <= MAX_PID_FILE))
            .filter_map(|file| {
                let text = fs::read_to_string(file.path()).ok()?;
                // Claude Code rewrites the file in place, so a shorter
                // version can leave the end of a longer one after it.
                serde_json::Deserializer::from_str(&text)
                    .into_iter::<PidFile>()
                    .next()?
                    .ok()
            })
            .collect()
    }
}

/// Claude Code's session ids are UUIDs; anything else is not looked up,
/// so an id never walks out of the store.
pub(super) fn is_session_id(id: &str) -> bool {
    id.len() == 36
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && id.matches('-').count() == 4
}

/// The folder name Claude Code files a folder's sessions under: every
/// character other than a letter or digit becomes `-`.
pub(crate) fn slug(folder: &Path) -> String {
    folder
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The newest `limit` sessions, or only `only`, newest first.
pub(super) fn list(
    store: &Store,
    snapshot: &Snapshot,
    limit: usize,
    only: Option<&str>,
) -> io::Result<Vec<ExternalSession>> {
    if let Some(id) = only.filter(|id| id.starts_with("subagent:")) {
        return super::subagents::session(store, id).map(|session| session.into_iter().collect());
    }

    // One session is looked up by its file, not by listing them all.
    if let Some(id) = only {
        return match store.find(id) {
            Some(path) => Ok(describe_file(store, &path, snapshot)?.into_iter().collect()),
            None => Ok(Vec::new()),
        };
    }

    let all = store.transcripts()?;
    describe_picked(store, snapshot, &all, all.iter().take(limit))
}

/// The sessions of `picked` among `all`, each told whether it is its
/// folder's newest.
fn describe_picked<'a>(
    store: &Store,
    snapshot: &Snapshot,
    all: &[Transcript],
    picked: impl Iterator<Item = &'a Transcript>,
) -> io::Result<Vec<ExternalSession>> {
    let newest_in_folder = newest_by_folder(all);
    let mut sessions = Vec::new();

    for transcript in picked {
        let newest = newest_in_folder
            .get(folder_of(transcript))
            .is_some_and(|id| *id == transcript.id);
        sessions.push(describe(store, transcript, newest, snapshot)?);
    }

    Ok(sessions)
}

/// The session whose transcript is `path`, `None` once the file is gone.
/// Only its own folder is read, for whether it is that folder's newest, and
/// only again once a file came to it or left it: a follow describes its file
/// on every update.
pub(super) fn describe_file(
    store: &Store,
    path: &Path,
    snapshot: &Snapshot,
) -> io::Result<Option<ExternalSession>> {
    let Some(transcript) = transcript_at(path) else {
        return Ok(None);
    };
    let newest = path
        .parent()
        .and_then(|folder| {
            store
                .memos
                .newest
                .read(folder, || newest_in(folder))
                .ok()
                .flatten()
        })
        .is_none_or(|(id, modified)| id == transcript.id || modified <= transcript.modified);
    describe(store, &transcript, newest, snapshot).map(Some)
}

/// The id and time of the newest transcript in `folder`.
fn newest_in(folder: &Path) -> io::Result<Option<(String, SystemTime)>> {
    Ok(fs::read_dir(folder)?
        .flatten()
        .filter_map(|file| transcript_at(&file.path()))
        .max_by_key(|other| other.modified)
        .map(|newest| (newest.id, newest.modified)))
}

/// The top-level transcript at `path`, when it is one.
fn transcript_at(path: &Path) -> Option<Transcript> {
    transcript_with(path, || fs::metadata(path))
}

/// `transcript_at`, with the file's metadata read by `metadata`.
fn transcript_with(
    path: &Path,
    metadata: impl FnOnce() -> io::Result<fs::Metadata>,
) -> Option<Transcript> {
    if path.extension().is_none_or(|ext| ext != "jsonl") {
        return None;
    }

    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| is_session_id(stem))?
        .to_owned();
    let meta = metadata().ok()?;
    Some(Transcript {
        id,
        path: path.to_owned(),
        modified: meta.modified().unwrap_or(UNIX_EPOCH),
        created: meta.created().ok(),
    })
}

/// The sessions named in `ids`, newest first.
pub(super) fn list_ids(
    store: &Store,
    snapshot: &Snapshot,
    ids: &HashSet<String>,
) -> io::Result<Vec<ExternalSession>> {
    let all = store.transcripts()?;
    describe_picked(
        store,
        snapshot,
        &all,
        all.iter().filter(|t| ids.contains(&t.id)),
    )
}

/// Every session, with the folder it ran in. Sessions are filed by that
/// folder, so only the head of each folder's newest transcript is read
/// (the next ones when it names no folder); at most `MAX_FOLDERS` folders.
pub(super) fn sightings(store: &Store) -> io::Result<Vec<Sighting>> {
    let all = store.transcripts()?;
    let mut by_folder: HashMap<&str, Vec<&Transcript>> = HashMap::new();

    for transcript in &all {
        by_folder
            .entry(folder_of(transcript))
            .or_default()
            .push(transcript);
    }

    let mut folders: Vec<Vec<&Transcript>> = by_folder.into_values().collect();
    // Newest first within each folder already; the busiest-lately first.
    folders.sort_by_key(|transcripts| Reverse(transcripts.first().map(|t| t.modified)));
    let mut out = Vec::new();

    for transcripts in folders.into_iter().take(MAX_FOLDERS) {
        let Some(cwd) = transcripts.iter().take(HEADS_PER_FOLDER).find_map(|t| {
            let read = || transcript::head(&t.path).map(|head| head.said.cwd);
            store.memos.cwds.read(&t.path, read).ok()?
        }) else {
            continue;
        };

        for transcript in transcripts {
            let modified = unix(transcript.modified);
            out.push(Sighting {
                agent: Agent::Claude,
                id: transcript.id.clone(),
                cwd: PathBuf::from(&cwd),
                started_at: transcript.created.map_or(modified, unix),
                last_activity: modified,
                title: None,
                file: Some(transcript.path.clone()),
            });
        }
    }

    Ok(out)
}

fn folder_of(transcript: &Transcript) -> &str {
    transcript
        .path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("")
}

/// The newest session id in each project folder; `all` is newest first.
fn newest_by_folder(all: &[Transcript]) -> HashMap<&str, &str> {
    let mut newest = HashMap::new();

    for transcript in all {
        newest
            .entry(folder_of(transcript))
            .or_insert(transcript.id.as_str());
    }

    newest
}

/// What `transcript` says, read from its tail (and its head for when it
/// started, when the file system does not say). After a plain append only
/// the new lines of a summary `before` are read.
fn summary(transcript: &Transcript, before: Option<Summary>) -> io::Result<Summary> {
    if let Some(before) = before
        && let Some(gist) = before.gist.appended(&transcript.path)?
    {
        return Ok(Summary { gist, ..before });
    }

    let gist = transcript::Gist::read(&transcript.path)?;
    let started_at = transcript
        .created
        .map(unix)
        .filter(|at| *at > 0)
        .or(transcript::started_at(&transcript.path)?)
        .unwrap_or(0);
    Ok(Summary { gist, started_at })
}

fn describe(
    store: &Store,
    transcript: &Transcript,
    newest_in_folder: bool,
    snapshot: &Snapshot,
) -> io::Result<ExternalSession> {
    let Summary { gist, started_at } = store
        .memos
        .summaries
        .read_since(&transcript.path, |before| summary(transcript, before))?;
    // Its last entry, not the file's time: lines written between turns
    // touch the file too.
    let active = gist.said.last_at.map_or(transcript.modified, |at| {
        UNIX_EPOCH + Duration::from_secs(at)
    });
    let found = presence(
        &Seen {
            id: &transcript.id,
            folder: folder_of(transcript),
            modified: active,
            newest_in_folder,
        },
        snapshot,
    );
    Ok(ExternalSession {
        id: transcript.id.clone(),
        agent: "claude".to_owned(),
        place: gist
            .said
            .cwd
            .as_deref()
            .and_then(|cwd| of_folder(Path::new(cwd))),
        title: gist.said.title(),
        started_at,
        last_activity: unix(active),
        presence: found.presence,
        pid: found.pid,
        last_prompt: gist.said.prompt().unwrap_or("").to_owned(),
        last_message: gist.said.last_message.unwrap_or_default(),
        cwd: gist.said.cwd.unwrap_or_default(),
        taken_over: snapshot.owned.get(&transcript.id).cloned(),
    })
}

/// A transcript, as `presence` needs it.
pub(super) struct Seen<'a> {
    pub(super) id: &'a str,
    /// Its project folder's name, the slug of the folder it ran in.
    pub(super) folder: &'a str,
    pub(super) modified: SystemTime,
    pub(super) newest_in_folder: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Found {
    pub(super) presence: Presence,
    pub(super) pid: Option<u32>,
}

/// Whether a process runs the session `seen`, and what it is doing.
pub(super) fn presence(seen: &Seen<'_>, snapshot: &Snapshot) -> Found {
    let running = |pid: u32| snapshot.processes.iter().any(|p| p.pid == pid);
    // Claude Code's own word, when its process still runs.
    if let Some(file) = snapshot
        .pid_files
        .iter()
        .find(|file| file.session_id == seen.id && running(file.pid))
    {
        let presence = match file.status.as_deref() {
            Some("busy") => Presence::Working,
            Some("idle") => Presence::Waiting,
            _ => Presence::Running,
        };
        return Found {
            presence,
            pid: Some(file.pid),
        };
    }

    let claimed = |pid: u32| {
        snapshot
            .pid_files
            .iter()
            .any(|file| file.pid == pid && running(file.pid))
    };
    // A process whose arguments name the session, one the daemon runs in
    // the background (it writes no pid file), while that is the newest in
    // its folder: after `/clear` the arguments still name the one before.
    if seen.newest_in_folder
        && let Some(process) = snapshot
            .processes
            .iter()
            .find(|process| process.session.as_deref() == Some(seen.id) && !claimed(process.pid))
    {
        return Found {
            presence: Presence::Running,
            pid: Some(process.pid),
        };
    }
    // A `claude` without a pid file in this session's folder, whatever its
    // arguments name, runs the folder's newest session.
    if seen.newest_in_folder
        && let Some(process) = snapshot.processes.iter().find(|process| {
            !claimed(process.pid)
                && process
                    .cwd
                    .as_deref()
                    .is_some_and(|cwd| slug(cwd) == seen.folder)
        })
    {
        return Found {
            presence: Presence::Running,
            pid: Some(process.pid),
        };
    }

    let now = snapshot.now.unwrap_or_else(SystemTime::now);
    let quiet = now.duration_since(seen.modified).unwrap_or_default();
    Found {
        presence: if quiet < RECENT {
            Presence::Recent
        } else {
            Presence::Stopped
        },
        pid: None,
    }
}

fn unix(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
#[path = "../../tests/agents_claude_discover.rs"]
mod tests;
