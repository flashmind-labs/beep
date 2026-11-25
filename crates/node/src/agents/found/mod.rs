//! The git checkouts coding agents worked in, from the sessions each agent
//! keeps on this machine (`Stores`), for suggesting projects to import and
//! listing an imported project's sessions.

use crate::agents::{place::of_folder, tags::title as prompt_title};
use beep_proto::SessionId;
use beep_proto::{
    AgentSlug,
    node::agent::{AgentSessions, ExternalSession, HistoryCursor, Presence, SessionFolder},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    cmp::Reverse,
    collections::HashMap,
    convert::Infallible,
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) mod agy;
mod codex;
mod codex_run;
mod delete;
mod disk;
mod flsh;
mod git;
pub(crate) mod memo;
mod opencode;

pub(crate) use codex::configured as codex_home;
pub(in crate::agents) use codex::is_rollout;
pub(in crate::agents) use flsh::databases as flsh_databases;
pub(crate) use flsh::sessions_dir as flsh_sessions_dir;
pub(crate) use opencode::databases_at as opencode_databases;
mod pi;
pub(crate) use pi::{agent_dir_in as pi_agent_dir, configured as pi_sessions};
mod transcript;

pub(crate) use delete::{remove_dir_in, remove_file_in};
pub(crate) use disk::{forget_checkouts, rescan, saved_checkouts, walk};
pub(crate) use git::{
    Checkout, checkout, common_dir, default_branch, git_dir, head, last_commit, place, repository,
};
pub(crate) use memo::Memo;

/// Session count used to size bounded metadata caches.
const MAX_FILES: usize = 2_000;
/// The most distinct folders looked up with git in one listing.
const MAX_LOOKUPS: usize = 400;
/// The longest first line read from a session file.
const MAX_FIRST_LINE: u64 = 512 * 1024;
/// A session written this recently may still run.
const RECENT: Duration = Duration::from_secs(3 * 60);

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
    pub(in crate::agents) const ALL: [Self; 6] = [
        Self::Claude,
        Self::Codex,
        Self::Pi,
        Self::Flsh,
        Self::OpenCode,
        Self::Agy,
    ];
    pub(in crate::agents) const STORED: [Self; 5] =
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

/// Where Codex, pi, flsh, OpenCode and Antigravity keep their sessions on
/// this machine (Claude Code's are in `crate::agents::claude`). A session's folder counts
/// for the checkout that holds it (`git`), so a subfolder or worktree counts
/// for the main folder. Every read is capped and blocks: run it with
/// `spawn_blocking`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stores {
    codex: Option<PathBuf>,
    pi: Option<PathBuf>,
    flsh: Option<PathBuf>,
    /// OpenCode's database, or its data folder of them.
    opencode: Option<PathBuf>,
    opencode_file: bool,
    /// Antigravity's conversations folder.
    agy: Option<PathBuf>,
    memos: Arc<Memos>,
    /// These are a WSL distribution's, read beside this machine's own.
    in_wsl: bool,
}

/// Resolves per-WSL-distribution state keyed by each distribution's Windows-mapped home.
pub(in crate::agents) fn in_wsl_homes<T: Clone>(
    kept: &std::sync::Mutex<HashMap<PathBuf, T>>,
    init: impl Fn(&Path) -> Option<T>,
) -> Vec<(crate::machine::wsl::DistroName, T)> {
    crate::machine::wsl::found()
        .into_iter()
        .filter_map(|(distro, found)| {
            let home = crate::machine::wsl::windows_path(&distro, &found.home?);
            if let Some(value) = crate::lock(kept).get(&home).cloned() {
                return Some((distro, value));
            }
            let value = init(&home)?;
            crate::lock(kept).insert(home, value.clone());
            Some((distro, value))
        })
        .collect()
}

/// The stores of the agents in each WSL distribution, kept by its home so
/// their memos last.
fn wsl_stores() -> Vec<(crate::machine::wsl::DistroName, Stores)> {
    static KEPT: std::sync::LazyLock<std::sync::Mutex<HashMap<PathBuf, Stores>>> =
        std::sync::LazyLock::new(Default::default);

    in_wsl_homes(&KEPT, |home| Some(Stores::in_wsl_home(home)))
}

/// What the stores' files said, kept while they are unchanged.
#[derive(Debug)]
struct Memos {
    /// Each Codex or pi session file's header.
    heads: Memo<Option<Seen>>,
    codex_children: Memo<Option<codex_run::Child>>,
    codex_runs: Memo<crate::agents::SubagentRun>,
    /// Each Codex or pi session file's name or first prompt.
    titles: Memo<Option<String>>,
    /// Each Codex session file's latest working folder.
    folders: Memo<Option<PathBuf>>,
    /// Codex's index of the names the user gave.
    names: Memo<Arc<HashMap<SessionId, String>>>,
    /// Each flsh or OpenCode database's sessions.
    databases: Memo<Arc<Vec<Seen>>>,
    /// Antigravity's titles and subagents.
    agy: Memo<Arc<agy::Summaries>>,
    /// The entries of the few Codex or pi session files read last.
    transcripts: Memo<Arc<Vec<beep_proto::node::agent::Entry>>>,
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

impl Stores {
    /// Configured stores, including ones a harness has not created yet.
    pub(crate) fn locate() -> Self {
        Self {
            codex: codex::configured(),
            pi: pi::configured(),
            flsh: flsh::sessions_dir(),
            opencode: opencode::configured(),
            opencode_file: opencode::configured_file(),
            agy: agy::conversations_dir(),
            memos: Arc::default(),
            in_wsl: false,
        }
    }

    /// The stores of the agents in a WSL distribution whose user's home is
    /// `home`, as Windows reaches it, where each keeps them by default.
    pub(crate) fn in_wsl_home(home: &Path) -> Self {
        Self {
            codex: Some(home.join(".codex")),
            pi: Some(home.join(".pi").join("agent").join("sessions")),
            flsh: Some(home.join(".local").join("share").join("flashmind").join("sessions")),
            opencode: Some(home.join(".local").join("share").join("opencode")),
            opencode_file: false,
            agy: Some(home.join(".gemini").join("antigravity-cli").join("conversations")),
            memos: Arc::default(),
            in_wsl: true,
        }
    }

    /// Store directories watched by open session subscriptions, including database WAL files.
    pub(crate) fn follow_paths(&self, agent: Option<Agent>) -> Vec<PathBuf> {
        let root = match agent {
            Some(Agent::Codex) => {
                return self
                    .codex
                    .as_ref()
                    .into_iter()
                    .flat_map(|root| [root.join("sessions"), root.join("session_index.jsonl")])
                    .collect();
            }
            Some(Agent::Pi) => self.pi.as_ref(),
            Some(Agent::Flsh) => self.flsh.as_ref(),
            Some(Agent::OpenCode) => self.opencode.as_ref(),
            Some(Agent::Agy) => self.agy.as_ref(),
            _ => None,
        };
        root.and_then(|path| {
            if path.is_file() || (agent == Some(Agent::OpenCode) && self.opencode_file) {
                path.parent()
            } else {
                Some(path.as_path())
            }
        })
        .map(PathBuf::from)
        .into_iter()
        .collect()
    }

    /// Every session in the stores, `agent`'s only when given.
    pub(crate) fn sessions(&self, agent: Option<Agent>) -> Vec<Seen> {
        let wants = |one: Agent| agent.is_none_or(|agent| agent == one);
        let mut seen = Vec::new();
        let memos = &self.memos;

        if let Some(root) = self.codex.as_deref()
            && wants(Agent::Codex)
        {
            seen.extend(codex::sessions(root, memos));
        }

        if let Some(root) = self.pi.as_deref()
            && wants(Agent::Pi)
        {
            seen.extend(pi::sessions(root, memos));
        }

        if let Some(root) = self.flsh.as_deref()
            && wants(Agent::Flsh)
        {
            seen.extend(flsh::sessions(root, memos));
        }

        if let Some(root) = self.opencode.as_deref()
            && wants(Agent::OpenCode)
        {
            seen.extend(opencode::sessions(root, memos));
        }

        if let Some(root) = self.agy.as_deref()
            && wants(Agent::Agy)
        {
            seen.extend(agy::sessions(root, memos));
        }

        self.with_wsl(seen, |stores| stores.sessions(agent))
    }

    /// Archived Codex conversations are searchable and can be opened by id.
    pub(crate) fn archived_codex(&self) -> Vec<Seen> {
        let seen = self
            .codex
            .as_deref()
            .map(|root| codex::archived(root, &self.memos))
            .unwrap_or_default();
        self.with_wsl(seen, Self::archived_codex)
    }

    fn with_wsl(&self, mut seen: Vec<Seen>, read: impl Fn(&Self) -> Vec<Seen>) -> Vec<Seen> {
        if !self.in_wsl {
            for (distro, stores) in wsl_stores() {
                for mut one in read(&stores) {
                    one.cwd =
                        crate::machine::wsl::windows_folder(&distro, &one.cwd.to_string_lossy());
                    seen.push(one);
                }
            }
        }

        seen
    }

    /// `seen` as `describe` lists it, its title read from its file when the
    /// store gave none.
    pub(crate) fn describe(&self, mut seen: Seen, now: SystemTime) -> ExternalSession {
        let native_run = seen
            .id
            .as_ref()
            .starts_with("subagent:")
            .then(|| {
                seen.file
                    .as_deref()
                    .and_then(|file| codex_run::read(file).ok())
            })
            .flatten();
        if seen.title.is_none()
            && let Some(file) = seen.file.as_deref()
        {
            let read = || Ok::<_, Infallible>(title(seen.agent, file));
            let Ok(title) = self.memos.titles.read(file, read);
            seen.title = title;
        }

        let folder = self.working_folder(&seen);
        let place = of_folder(&folder);
        let mut session = ExternalSession {
            place,
            ..describe(seen, now)
        };

        if let Some(run) = native_run {
            session.presence = if run.working {
                Presence::Working
            } else {
                Presence::Stopped
            };
            session.last_message = run.result.unwrap_or_default();
        }

        session
    }

    /// The folder `seen` works in now: what its agent last recorded, else
    /// the folder it started in. Codex's rollout says; the others keep only the folder a session started in.
    fn working_folder(&self, seen: &Seen) -> PathBuf {
        let recorded = match (seen.agent, seen.file.as_deref()) {
            (Agent::Codex, Some(file)) => {
                let read = || Ok::<_, Infallible>(codex::latest_folder(file));
                let Ok(folder) = self.memos.folders.read(file, read);
                folder
            }
            _ => None,
        };
        let folder = recorded.unwrap_or_else(|| seen.cwd.clone());
        // A Codex file in WSL names its folders as Linux does.
        crate::machine::wsl::windows_drive_path(&folder.to_string_lossy())
            .map_or(folder, PathBuf::from)
    }

    /// Direct Codex children, including task-path spawns not exposed over ACP.
    pub(crate) fn codex_children(&self, parent: &SessionId) -> Vec<crate::agents::SubagentRun> {
        let Some(root) = self.codex.as_deref() else {
            return Vec::new();
        };
        let mut files = newest_files(&root.join("sessions"), 3, &codex::is_rollout);
        files.sort_by(|(left, _), (right, _)| left.cmp(right));
        files
            .into_iter()
            .filter_map(|(path, _)| {
                let child = self
                    .memos
                    .codex_children
                    .read(&path, || {
                        first_line(&path).map(|line| line.as_deref().and_then(codex_run::parent))
                    })
                    .ok()??;
                if child.parent != *parent {
                    return None;
                }
                self.memos
                    .codex_runs
                    .read(&path, || codex_run::read(&path))
                    .ok()
                    .filter(|run| run.session.as_ref() == Some(&child.session))
            })
            .collect()
    }

    /// What the first of `threads` with a rollout here did, for a Codex
    /// subagent's entry; `None` when Codex has no folder or no rollout of
    /// them.
    pub(crate) fn codex_run(&self, threads: &[String]) -> Option<crate::agents::SubagentRun> {
        let root = self.codex.as_deref()?;
        let path = threads
            .iter()
            .find_map(|thread| codex_run::rollout(root, thread))?;
        self.memos
            .codex_runs
            .read(&path, || codex_run::read(&path))
            .inspect_err(|e| {
                log::debug!(
                    "subagent rollout not read; path={} error={e}",
                    path.display()
                )
            })
            .ok()
    }

    /// The file of session `id`, of `agent` when given; `None` for one not
    /// found or kept in a database.
    pub(crate) fn file(&self, agent: Option<Agent>, id: &SessionId) -> Option<PathBuf> {
        self.seen(agent, id)?.file
    }

    pub(super) fn seen(&self, agent: Option<Agent>, id: &SessionId) -> Option<Seen> {
        if id.as_ref().starts_with("subagent:") {
            return agent
                .is_none_or(|agent| agent == Agent::Codex)
                .then(|| codex::native(self.codex.as_deref()?, id))
                .flatten();
        }

        self.sessions(agent)
            .into_iter()
            .find(|seen| seen.id == *id)
            .or_else(|| {
                if agent.is_some_and(|agent| agent != Agent::Codex) {
                    return None;
                }

                self.archived_codex().into_iter().find(|seen| seen.id == *id)
            })
    }

    /// One session of `agent` by its id, described.
    pub(crate) fn find(&self, agent: Agent, id: &SessionId) -> Option<ExternalSession> {
        let seen = self.seen(Some(agent), id)?;
        Some(self.describe(seen, SystemTime::now()))
    }

    #[cfg(test)]
    pub(crate) fn at(codex: Option<&Path>, pi: Option<&Path>, flsh: Option<&Path>) -> Self {
        Self {
            codex: codex.map(Path::to_owned),
            pi: pi.map(Path::to_owned),
            flsh: flsh.map(Path::to_owned),
            ..Self::default()
        }
    }
}

/// Where Codex, pi, flsh and OpenCode keep their sessions here, for
/// reading what they spent (`crate::agents::spend`).
#[derive(Debug, Clone, Default)]
pub(crate) struct LogRoots {
    /// `CODEX_HOME`; its sessions are in `sessions/`.
    pub(crate) codex: Option<PathBuf>,
    pub(crate) pi: Option<PathBuf>,
    pub(crate) flsh: Option<PathBuf>,
    /// OpenCode's database, or its data folder of them.
    pub(crate) opencode: Option<PathBuf>,
    /// pi's agent dir, which holds its `auth.json`.
    pub(crate) pi_agent: Option<PathBuf>,
}

impl LogRoots {
    pub(crate) fn locate() -> Self {
        Self {
            codex: codex::locate(),
            pi: pi::locate(),
            flsh: flsh::locate(),
            opencode: opencode::locate(),
            pi_agent: pi::agent_dir(),
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

/// Sessions in one folder, per agent, as `folders` gathers them.
#[derive(Debug, Default)]
struct Tally {
    agents: HashMap<Agent, u32>,
    last_activity: u64,
}

impl Tally {
    fn add(&mut self, agent: Agent, sessions: u32, last_activity: u64) {
        *self.agents.entry(agent).or_default() += sessions;
        self.last_activity = self.last_activity.max(last_activity);
    }
}

/// The checkouts `seen` ran in, newest activity first, at most `limit`.
/// `home` and the root, what Beep keeps hidden in `home`, and folders that
/// are gone or not in a checkout, are left out. `lookup` finds a folder's checkout, as `checkout` does.
pub(crate) fn folders(
    seen: &[Seen],
    home: Option<&Path>,
    limit: usize,
    mut lookup: impl FnMut(&Path) -> Option<Checkout>,
) -> Vec<SessionFolder> {
    let mut by_cwd: HashMap<&Path, Tally> = HashMap::new();

    for one in seen {
        by_cwd
            .entry(one.cwd.as_path())
            .or_default()
            .add(one.agent, 1, one.last_activity);
    }

    let mut cwds: Vec<(&Path, Tally)> = by_cwd.into_iter().collect();
    cwds.sort_by_key(|(cwd, tally)| (Reverse(tally.last_activity), *cwd));
    let hidden = home.map(|home| home.join(super::conversation::HIDDEN));
    let skipped = |path: &Path| {
        path.parent().is_none()
            || Some(path) == home
            || hidden.as_ref().is_some_and(|hidden| path.starts_with(hidden))
    };
    let mut roots: HashMap<PathBuf, (String, Tally)> = HashMap::new();

    // The skipped ones take none of the lookups: a run of chats in their
    // own folders leaves the checkouts their turn.
    let looked = cwds.into_iter().filter(|(cwd, _)| !skipped(cwd));

    for (cwd, tally) in looked.take(MAX_LOOKUPS) {
        if !cwd.is_absolute() || !cwd.is_dir() {
            continue;
        }

        let Some(found) = lookup(cwd) else {
            continue;
        };

        if skipped(&found.root) {
            continue;
        }

        let (_, into) = roots
            .entry(found.root)
            .or_insert_with(|| (found.origin, Tally::default()));
        for (agent, sessions) in tally.agents {
            into.add(agent, sessions, tally.last_activity);
        }
    }

    let mut out: Vec<SessionFolder> = roots
        .into_iter()
        .map(|(root, (origin, tally))| {
            let mut agents: Vec<(Agent, u32)> = tally.agents.into_iter().collect();
            agents.sort_by_key(|(agent, sessions)| (Reverse(*sessions), *agent));
            SessionFolder {
                path: root.to_string_lossy().into_owned(),
                origin,
                agents: agents
                    .into_iter()
                    .map(|(agent, sessions)| AgentSessions {
                        agent: agent.to_string(),
                        sessions,
                    })
                    .collect(),
                last_activity: tally.last_activity,
            }
        })
        .collect();
    out.sort_by(newest_first);
    out.truncate(limit);
    out
}

/// Most recent activity first, then by path.
fn newest_first(a: &SessionFolder, b: &SessionFolder) -> std::cmp::Ordering {
    b.last_activity
        .cmp(&a.last_activity)
        .then_with(|| a.path.cmp(&b.path))
}

/// Adds to `out` each of `checkouts` it does not hold yet, with no
/// sessions, until it holds `limit`. `home` is left out.
pub(crate) fn add_checkouts(
    out: &mut Vec<SessionFolder>,
    checkouts: &[Checkout],
    home: Option<&Path>,
    limit: usize,
) {
    for one in checkouts {
        if out.len() >= limit {
            break;
        }

        let path = one.root.to_string_lossy();

        if Some(one.root.as_path()) == home || out.iter().any(|folder| folder.path == path) {
            continue;
        }

        out.push(SessionFolder {
            path: path.into_owned(),
            origin: one.origin.clone(),
            agents: Vec::new(),
            last_activity: 0,
        });
    }
}

/// Raises each folder's `last_activity` to its last commit when that is
/// newer (`commit` reads it, as `last_commit` does), then sorts newest first.
pub(crate) fn with_commits(out: &mut [SessionFolder], commit: impl Fn(&Path) -> Option<u64>) {
    for folder in out.iter_mut() {
        if let Some(at) = commit(Path::new(&folder.path)) {
            folder.last_activity = folder.last_activity.max(at);
        }
    }

    out.sort_by(newest_first);
}

/// Whether `cwd` is `folder` or inside it.
pub(crate) fn within(cwd: &Path, folder: &Path) -> bool {
    folder.parent().is_some() && cwd.starts_with(folder)
}

/// The sessions of `seen` in any of `folders`, newest first, at most
/// `limit`.
pub(crate) fn in_folders(mut seen: Vec<Seen>, folders: &[PathBuf], limit: usize) -> Vec<Seen> {
    seen.retain(|one| folders.iter().any(|folder| within(&one.cwd, folder)));
    seen.sort_by_key(|one| Reverse(one.last_activity));
    seen.truncate(limit);
    seen
}

/// A stable page boundary also distinguishes sessions with the same activity time.
pub(crate) fn history_page(
    mut seen: Vec<Seen>,
    folders: &[PathBuf],
    limit: usize,
    before: Option<&HistoryCursor>,
) -> (Vec<Seen>, Option<HistoryCursor>) {
    let position = |seen: &Seen| HistoryCursor {
        last_activity: seen.last_activity,
        agent: seen.agent.to_string(),
        id: seen.id.clone(),
    };
    fn key(seen: &Seen) -> (Reverse<u64>, &str, &SessionId) {
        (Reverse(seen.last_activity), seen.agent.as_ref(), &seen.id)
    }
    seen.retain(|one| {
        folders.iter().any(|folder| within(&one.cwd, folder))
            && before.is_none_or(|before| {
                key(one)
                    > (
                        Reverse(before.last_activity),
                        before.agent.as_str(),
                        &before.id,
                    )
            })
    });
    seen.sort_by(|a, b| key(a).cmp(&key(b)));
    seen.dedup_by(|a, b| a.agent == b.agent && a.id == b.id);
    let more = seen.len() > limit;
    seen.truncate(limit);
    let next = if more {
        seen.last().map(position)
    } else {
        None
    };
    (seen, next)
}

/// The name or first prompt in `agent`'s session file at `file`.
fn title(agent: Agent, file: &Path) -> Option<String> {
    match agent {
        Agent::Codex => codex::title(file),
        Agent::Pi => pi::title(file),
        Agent::Claude | Agent::Flsh | Agent::OpenCode | Agent::Agy => None,
    }
}

/// `seen` as the server lists a session started outside Beep: its title
/// (`Stores::describe` reads one from the file), and whether it may still
/// run.
fn describe(seen: Seen, now: SystemTime) -> ExternalSession {
    let modified = UNIX_EPOCH + Duration::from_secs(seen.last_activity);
    ExternalSession {
        id: seen.id,
        agent: seen.agent.to_string(),
        cwd: seen.cwd.to_string_lossy().into_owned(),
        title: seen.title,
        started_at: seen.started_at,
        last_activity: seen.last_activity,
        presence: quiet_presence(modified, now),
        ..ExternalSession::default()
    }
}

pub(in crate::agents) fn quiet_presence(modified: SystemTime, now: SystemTime) -> Presence {
    if now.duration_since(modified).unwrap_or_default() < RECENT {
        Presence::Recent
    } else {
        Presence::Stopped
    }
}

fn xdg_data_dir(home: Option<PathBuf>) -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.map(|home| home.join(".local/share")))
}

fn read_databases(
    memos: &Memos,
    databases: impl IntoIterator<Item = PathBuf>,
    read: fn(&Path) -> rusqlite::Result<Vec<Seen>>,
    name: &str,
) -> Vec<Seen> {
    let mut seen = Vec::new();

    for db in databases {
        match memos.databases.read_database(&db, || read(&db).map(Arc::new)) {
            Ok(found) => seen.extend(found.iter().cloned()),
            Err(e) => log::debug!("{name} sessions not read; database={} error={e}", db.display()),
        }
    }

    seen
}

/// The first line of `path`, when it is not too long to be a header.
fn first_line(path: &Path) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    BufReader::new(File::open(path)?.take(MAX_FIRST_LINE)).read_until(b'\n', &mut line)?;

    if !line.ends_with(b"\n") && line.len() as u64 >= MAX_FIRST_LINE {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&line).into_owned()))
}

pub(in crate::agents) fn unix_secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Unix seconds of an RFC 3339 UTC time such as `2026-10-29T12:34:56.789Z`.
pub(in crate::agents) fn unix_seconds(text: &str) -> Option<u64> {
    unix_millis(text).map(|ms| ms / 1000)
}

/// Unix milliseconds of an RFC 3339 time such as `2026-10-29T12:34:56.789Z`.
pub(in crate::agents) fn unix_millis(text: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .and_then(|at| u64::try_from(at.timestamp_millis()).ok())
}

pub(in crate::agents) fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
}

pub(in crate::agents) fn wal_path(path: &Path) -> PathBuf {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    PathBuf::from(wal)
}

fn header_sessions(
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
fn newest_files(dir: &Path, depth: usize, wanted: &dyn Fn(&Path) -> bool) -> Vec<(PathBuf, u64)> {
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
pub(in crate::agents) fn walk_files(
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

#[cfg(test)]
#[path = "../../tests/agents_found.rs"]
mod tests;
