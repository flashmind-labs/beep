//! Finding Claude Code sessions on the machine (`Store`, `list`,
//! `sightings`): transcripts under `<config>/projects/`, and which of them a
//! running `claude` has open.

use super::{procs::Process, transcript};
use crate::agents::found::{Agent, Memo, Seen as Sighting};
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
}

impl Default for Memos {
    fn default() -> Self {
        Self {
            cwds: Memo::new(MAX_FOLDERS * HEADS_PER_FOLDER * 2),
            summaries: Memo::new(MAX_SUMMARIES),
        }
    }
}

/// What a transcript says, as a listing shows it.
#[derive(Debug, Clone)]
struct Summary {
    cwd: Option<String>,
    title: Option<String>,
    last_prompt: String,
    last_message: String,
    /// Unix seconds.
    started_at: u64,
    /// Unix seconds of its last timestamped line. Claude Code also appends
    /// lines without one, such as its artifact ledger, between turns.
    last_at: Option<u64>,
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
#[derive(Debug, Default)]
pub(super) struct Snapshot {
    pub(super) processes: Vec<Process>,
    pub(super) pid_files: Vec<PidFile>,
    /// Sessions this node runs: the agent's id to the node's.
    pub(super) owned: HashMap<String, String>,
    pub(super) now: Option<SystemTime>,
}

impl Store {
    /// The store on this machine, when Claude Code has one.
    pub(crate) fn locate() -> Option<Self> {
        let root = super::config_dir()?;
        root.join("projects").is_dir().then(|| Self {
            root,
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
        for project in fs::read_dir(self.root.join("projects"))? {
            let project = project?.path();
            let Ok(files) = fs::read_dir(&project) else {
                continue;
            };
            for file in files.flatten() {
                if found.len() >= MAX_FILES {
                    break;
                }
                let path = file.path();
                if path.extension().is_none_or(|ext| ext != "jsonl") {
                    continue;
                }
                let Some(id) = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| is_session_id(stem))
                    .map(str::to_owned)
                else {
                    continue;
                };
                let Ok(meta) = file.metadata() else {
                    continue;
                };
                found.push(Transcript {
                    id,
                    path,
                    modified: meta.modified().unwrap_or(UNIX_EPOCH),
                    created: meta.created().ok(),
                });
            }
        }
        found.sort_by_key(|transcript| Reverse(transcript.modified));
        Ok(found)
    }

    /// The transcript of session `id`.
    pub(super) fn find(&self, id: &str) -> Option<PathBuf> {
        if !is_session_id(id) {
            return None;
        }
        let name = format!("{id}.jsonl");
        fs::read_dir(self.root.join("projects"))
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
fn slug(folder: &Path) -> String {
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
    let all = store.transcripts()?;
    let newest_in_folder = newest_by_folder(&all);
    let picked: Vec<&Transcript> = match only {
        Some(id) => all.iter().filter(|t| t.id == id).collect(),
        None => all.iter().take(limit).collect(),
    };
    let mut sessions = Vec::new();
    for transcript in picked {
        let newest = newest_in_folder
            .get(folder_of(transcript))
            .is_some_and(|id| *id == transcript.id);
        sessions.push(describe(store, transcript, newest, snapshot)?);
    }
    Ok(sessions)
}

/// The sessions named in `ids`, newest first.
pub(super) fn list_ids(
    store: &Store,
    snapshot: &Snapshot,
    ids: &HashSet<String>,
) -> io::Result<Vec<ExternalSession>> {
    let all = store.transcripts()?;
    let newest_in_folder = newest_by_folder(&all);
    let mut sessions = Vec::new();
    for transcript in all.iter().filter(|t| ids.contains(&t.id)) {
        let newest = newest_in_folder
            .get(folder_of(transcript))
            .is_some_and(|id| *id == transcript.id);
        sessions.push(describe(store, transcript, newest, snapshot)?);
    }
    Ok(sessions)
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
            let read = || transcript::head(&t.path).map(|head| head.cwd);
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
/// started, when the file system does not say).
fn summary(transcript: &Transcript) -> io::Result<Summary> {
    let digest = transcript::digest_tail(&transcript.path, 0)?;
    let started_at = transcript
        .created
        .map(unix)
        .filter(|at| *at > 0)
        .or(transcript::started_at(&transcript.path)?)
        .unwrap_or(0);
    Ok(Summary {
        title: digest.title().map(str::to_owned),
        last_prompt: digest.prompt().unwrap_or("").to_owned(),
        last_message: digest.last_message.clone().unwrap_or_default(),
        cwd: digest.cwd,
        started_at,
        last_at: digest.last_at,
    })
}

fn describe(
    store: &Store,
    transcript: &Transcript,
    newest_in_folder: bool,
    snapshot: &Snapshot,
) -> io::Result<ExternalSession> {
    let summary = store
        .memos
        .summaries
        .read(&transcript.path, || summary(transcript))?;
    // Its last entry, not the file's time: lines written between turns
    // touch the file too.
    let active = summary.last_at.map_or(transcript.modified, |at| {
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
        cwd: summary.cwd.unwrap_or_default(),
        title: summary.title,
        started_at: summary.started_at,
        last_activity: unix(active),
        presence: found.presence,
        pid: found.pid,
        last_prompt: summary.last_prompt,
        last_message: summary.last_message,
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
    // A `claude` without a pid file, in this session's folder.
    let claimed = |pid: u32| {
        snapshot
            .pid_files
            .iter()
            .any(|file| file.pid == pid && running(file.pid))
    };
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
mod tests {
    use super::*;
    use crate::agents::claude::transcript::tests::FIXTURE;

    const ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";
    const OTHER: &str = "7a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";

    fn seen(modified: SystemTime, newest: bool) -> Seen<'static> {
        Seen {
            id: ID,
            folder: "-srv-project",
            modified,
            newest_in_folder: newest,
        }
    }

    fn process(pid: u32, cwd: &str) -> Process {
        Process {
            pid,
            cwd: Some(PathBuf::from(cwd)),
        }
    }

    #[test]
    fn a_pid_file_names_the_session_and_its_state() {
        let now = SystemTime::now();
        let snapshot = Snapshot {
            processes: vec![process(41, "/elsewhere")],
            pid_files: vec![PidFile {
                pid: 41,
                session_id: ID.into(),
                status: Some("busy".into()),
            }],
            now: Some(now),
            ..Snapshot::default()
        };
        let old = now - Duration::from_secs(3600);
        assert_eq!(
            presence(&seen(old, false), &snapshot),
            Found {
                presence: Presence::Working,
                pid: Some(41)
            }
        );
    }

    #[test]
    fn a_stale_pid_file_does_not_count() {
        let now = SystemTime::now();
        let snapshot = Snapshot {
            pid_files: vec![PidFile {
                pid: 41,
                session_id: ID.into(),
                status: Some("idle".into()),
            }],
            now: Some(now),
            ..Snapshot::default()
        };
        let old = now - Duration::from_secs(3600);
        assert_eq!(
            presence(&seen(old, true), &snapshot).presence,
            Presence::Stopped
        );
        let fresh = now - Duration::from_secs(30);
        assert_eq!(
            presence(&seen(fresh, true), &snapshot).presence,
            Presence::Recent
        );
    }

    #[test]
    fn a_process_in_the_folder_runs_its_newest_session_only() {
        let now = SystemTime::now();
        let old = now - Duration::from_secs(3600);
        let snapshot = Snapshot {
            processes: vec![process(7, "/srv/project")],
            now: Some(now),
            ..Snapshot::default()
        };
        assert_eq!(
            presence(&seen(old, true), &snapshot),
            Found {
                presence: Presence::Running,
                pid: Some(7)
            }
        );
        assert_eq!(
            presence(&seen(old, false), &snapshot).presence,
            Presence::Stopped
        );
        // A process whose pid file names another session is not this one.
        let claimed = Snapshot {
            pid_files: vec![PidFile {
                pid: 7,
                session_id: OTHER.into(),
                status: None,
            }],
            ..snapshot
        };
        assert_eq!(
            presence(&seen(old, true), &claimed).presence,
            Presence::Stopped
        );
    }

    #[test]
    fn folders_slug_like_claude_code() {
        assert_eq!(
            slug(Path::new("/srv/my_project/.worktrees/v2")),
            "-srv-my-project--worktrees-v2"
        );
    }

    #[test]
    fn a_store_lists_sessions_newest_first_with_what_they_say() {
        let root = std::env::temp_dir().join(format!("beep-claude-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("projects").join("-srv-project");
        fs::create_dir_all(&project).unwrap();
        let older = project.join(format!("{OTHER}.jsonl"));
        fs::write(&older, "").unwrap();
        fs::File::options()
            .write(true)
            .open(&older)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(600))
            .unwrap();
        fs::write(project.join(format!("{ID}.jsonl")), FIXTURE).unwrap();
        fs::write(project.join("notes.jsonl"), "").unwrap();
        fs::create_dir_all(root.join("sessions")).unwrap();
        fs::write(
            root.join("sessions").join("41.json"),
            // Left over from a longer earlier version of the file.
            format!(
                r#"{{"pid":41,"sessionId":"{ID}","status":"idle","cwd":"/srv/project"}}t":1}}"#
            ),
        )
        .unwrap();
        let store = Store::at(&root);
        let snapshot = Snapshot {
            processes: vec![process(41, "/srv/project")],
            pid_files: store.pid_files(),
            owned: HashMap::from([(OTHER.to_owned(), "a3f9c1".to_owned())]),
            now: None,
        };
        let sessions = list(&store, &snapshot, 10, None).unwrap();
        assert_eq!(sessions.len(), 2);
        let first = &sessions[0];
        assert_eq!(first.id, ID);
        assert_eq!(first.cwd, "/srv/project");
        assert_eq!(first.title.as_deref(), Some("Fix the flaky test"));
        assert_eq!(first.presence, Presence::Waiting);
        assert_eq!(first.pid, Some(41));
        assert_eq!(first.last_prompt, "now run the whole suite");
        assert_eq!(first.last_message, "All 42 tests pass now.");
        assert_eq!(sessions[1].taken_over.as_deref(), Some("a3f9c1"));
        assert_eq!(list(&store, &snapshot, 10, Some(OTHER)).unwrap().len(), 1);
        assert!(store.find(ID).is_some());
        assert!(store.find("../x").is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_transcript_is_read_again_only_once_it_changed() {
        let root = tempfile::tempdir().expect("a temp folder");
        let project = root.path().join("projects").join("-srv-project");
        fs::create_dir_all(&project).expect("folders");
        let path = project.join(format!("{ID}.jsonl"));
        fs::write(&path, FIXTURE).expect("a transcript");
        let store = Store::at(root.path());
        let snapshot = Snapshot::default();
        let ids = HashSet::from([ID.to_owned()]);
        let prompt = |store: &Store| {
            let listed = list_ids(store, &snapshot, &ids).expect("the listing");
            listed[0].last_prompt.clone()
        };
        assert_eq!(prompt(&store), "now run the whole suite");

        // The same length and modification time: taken as unchanged.
        let modified = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .expect("a modification time");
        fs::write(&path, FIXTURE.replace("whole suite", "whole thing")).expect("a rewrite");
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(modified))
            .expect("the old time back");
        assert_eq!(prompt(&store), "now run the whole suite");
        assert_eq!(
            prompt(&Store::at(root.path())),
            "now run the whole thing",
            "a new store reads the file"
        );

        fs::write(&path, FIXTURE.replace("whole suite", "net tests")).expect("a change");
        assert_eq!(prompt(&store), "now run the net tests");
    }

    /// Lines Claude Code writes between turns, such as its artifact
    /// ledger, touch the file but are not activity: the session's time is
    /// its last entry's, and nothing running it shows it stopped.
    #[test]
    fn lines_between_turns_do_not_make_a_session_recent() {
        let root = tempfile::tempdir().expect("a temp folder");
        let project = root.path().join("projects").join("-srv-project");
        fs::create_dir_all(&project).expect("folders");
        let ledger = r#"{"type":"artifact-autoreact-ledger","v":1,"artifacts":{}}"#;
        let text = format!("{FIXTURE}{ledger}\n{ledger}\n");
        fs::write(project.join(format!("{ID}.jsonl")), text).expect("a transcript");
        let store = Store::at(root.path());
        let listed = list(&store, &Snapshot::default(), 10, None).expect("the listing");
        // 2026-10-29T08:03:45Z, the fixture's last entry.
        assert_eq!(listed[0].last_activity, 1_793_261_025);
        assert_eq!(listed[0].presence, Presence::Stopped);
    }
}
