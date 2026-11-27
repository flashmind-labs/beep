//! What every store reader shares: the agents, a session as a store lists
//! it, what the readers keep while files are unchanged, and the small
//! readers of files and times.

use super::memo::Memo;
use beep_proto::{AgentSlug, SessionId, node::agent::Presence};
use rusqlite::{Connection, OpenFlags};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) use crate::tags::title as prompt_title;

/// Session count used to size bounded metadata caches.
pub(super) const MAX_FILES: usize = 2_000;
/// The longest first line read from a session file.
pub(super) const MAX_FIRST_LINE: u64 = 512 * 1024;
/// A session written this recently may still run.
pub(super) const RECENT: Duration = Duration::from_secs(3 * 60);

/// A coding agent whose sessions are read here.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    strum::AsRefStr,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub(crate) enum Agent {
    Claude,
    Codex,
    Pi,
    Flsh,
    OpenCode,
    Agy,
}

impl Agent {
    pub(crate) const ALL: [Self; 6] = [
        Self::Claude,
        Self::Codex,
        Self::Pi,
        Self::Flsh,
        Self::OpenCode,
        Self::Agy,
    ];
    pub(crate) const STORED: [Self; 5] =
        [Self::Codex, Self::Pi, Self::Flsh, Self::OpenCode, Self::Agy];
}

impl From<Agent> for AgentSlug {
    fn from(agent: Agent) -> Self {
        Self::from(agent.as_ref())
    }
}

/// One session as its store lists it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seen {
    pub(crate) agent: Agent,
    /// The agent's own id for it.
    pub(crate) id: SessionId,
    /// The folder it ran in.
    pub(crate) cwd: PathBuf,
    /// Unix seconds.
    pub(crate) started_at: u64,
    pub(crate) last_activity: u64,
    /// Its title or first prompt, when the store gave it cheaply.
    pub(crate) title: Option<String>,
    /// Its file, read for the title when `title` is not known yet.
    pub(crate) file: Option<PathBuf>,
}

/// What the stores' files said, kept while they are unchanged.
#[derive(Debug)]
pub(crate) struct Memos {
    /// Each Codex or pi session file's header.
    pub(super) heads: Memo<Option<Seen>>,
    pub(super) codex_children: Memo<Option<Child>>,
    pub(super) codex_runs: Memo<crate::logs::subagent::Run>,
    /// Each Codex or pi session file's name or first prompt.
    pub(super) titles: Memo<Option<String>>,
    /// Each Codex session file's latest working folder.
    pub(super) folders: Memo<Option<PathBuf>>,
    /// Codex's index of the names the user gave.
    pub(super) names: Memo<Arc<HashMap<SessionId, String>>>,
    /// Each flsh or OpenCode database's sessions.
    pub(super) databases: Memo<Arc<Vec<Seen>>>,
    /// Antigravity's titles and subagents.
    pub(super) agy: Memo<Arc<Summaries>>,
    /// The entries of the few Codex or pi session files read last.
    pub(super) transcripts: Memo<Arc<Vec<beep_proto::node::agent::Entry>>>,
}

/// How many sessions' parsed transcripts are kept at once.
const TRANSCRIPTS_KEPT: usize = 4;

impl Default for Memos {
    fn default() -> Self {
        Self {
            heads: Memo::new(MAX_FILES * 4),
            codex_children: Memo::new(MAX_FILES * 4),
            codex_runs: Memo::new(MAX_FILES),
            titles: Memo::new(1_000),
            folders: Memo::new(1_000),
            names: Memo::new(4),
            databases: Memo::new(1_000),
            agy: Memo::new(4),
            transcripts: Memo::new(TRANSCRIPTS_KEPT),
        }
    }
}

/// Opens the agent's database `path` to read, waiting a little while it is
/// locked.
pub(crate) fn open_database(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_millis(200))?;
    Ok(conn)
}

pub(crate) fn quiet_presence(modified: SystemTime, now: SystemTime) -> Presence {
    if now.duration_since(modified).unwrap_or_default() < RECENT {
        Presence::Recent
    } else {
        Presence::Stopped
    }
}

pub(super) fn xdg_data_dir(home: Option<PathBuf>) -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.map(|home| home.join(".local/share")))
}

pub(super) fn read_databases(
    memos: &Memos,
    databases: impl IntoIterator<Item = PathBuf>,
    read: fn(&Path) -> rusqlite::Result<Vec<Seen>>,
    name: &str,
) -> Vec<Seen> {
    let mut seen = Vec::new();

    for db in databases {
        match memos
            .databases
            .read_database(&db, || read(&db).map(Arc::new))
        {
            Ok(found) => seen.extend(found.iter().cloned()),
            Err(e) => log::debug!(
                "{name} sessions not read; database={} error={e}",
                db.display()
            ),
        }
    }

    seen
}

/// The first line of `path`, when it is not too long to be a header.
pub(crate) fn first_line(path: &Path) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    BufReader::new(File::open(path)?.take(MAX_FIRST_LINE)).read_until(b'\n', &mut line)?;

    if !line.ends_with(b"\n") && line.len() as u64 >= MAX_FIRST_LINE {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&line).into_owned()))
}

pub(crate) fn unix_secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Unix seconds of an RFC 3339 UTC time such as `2026-10-29T12:34:56.789Z`.
pub(crate) fn unix_seconds(text: &str) -> Option<u64> {
    unix_millis(text).map(|ms| ms / 1000)
}

/// Unix milliseconds of an RFC 3339 time such as `2026-10-29T12:34:56.789Z`.
pub(crate) fn unix_millis(text: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .and_then(|at| u64::try_from(at.timestamp_millis()).ok())
}

pub(crate) fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
}

pub(super) fn header_sessions(
    dir: &Path,
    depth: usize,
    wanted: &dyn Fn(&Path) -> bool,
    heads: &Memo<Option<Seen>>,
    header: fn(&str) -> Option<Seen>,
) -> impl Iterator<Item = Seen> {
    newest_files(dir, depth, wanted)
        .into_iter()
        .filter_map(move |(path, at)| {
            let mut seen = heads
                .read(&path, || {
                    first_line(&path).map(|line| line.as_deref().and_then(header))
                })
                .ok()??;
            seen.last_activity = at;
            seen.file = Some(path);
            Some(seen)
        })
}

/// The files under `dir` for which `wanted` holds,
/// `depth` folders down, newest first.
pub(crate) fn newest_files(
    dir: &Path,
    depth: usize,
    wanted: &dyn Fn(&Path) -> bool,
) -> Vec<(PathBuf, u64)> {
    let mut found = Vec::new();
    walk_files(dir, depth, wanted, &mut |entry, path| {
        let at = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .map_or(0, unix_secs);
        found.push((path, at));
        true
    });
    found.sort_by_key(|(_, at)| Reverse(*at));
    found
}

/// Visits regular files `depth` folders down under `dir` matching `wanted`,
/// without following directory symlinks. Stops early when `visit` returns `false`.
pub(crate) fn walk_files(
    dir: &Path,
    depth: usize,
    wanted: &dyn Fn(&Path) -> bool,
    visit: &mut dyn FnMut(&std::fs::DirEntry, PathBuf) -> bool,
) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if depth > 0 && kind.is_dir() {
            if !walk_files(&path, depth - 1, wanted, visit) {
                return false;
            }
        } else if depth == 0 && kind.is_file() && wanted(&path) && !visit(&entry, path) {
            return false;
        }
    }

    true
}

#[derive(Debug, Clone)]
pub(crate) struct Child {
    pub(crate) parent: SessionId,
    pub(crate) session: SessionId,
}

/// What `conversation_summaries.db` says of the conversations.
#[derive(Debug, Default)]
pub(crate) struct Summaries {
    pub(super) titles: HashMap<SessionId, String>,
    /// Conversations a session started, as subagents.
    pub(super) children: HashSet<SessionId>,
}
