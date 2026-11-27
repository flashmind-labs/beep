//! The git checkouts coding agents worked in, from the sessions each agent
//! keeps on this machine (`Stores`), for suggesting projects to import and
//! listing an imported project's sessions.

use folder_place::of_folder;
use beep_proto::SessionId;
use beep_proto::{
    node::agent::{AgentSessions, ExternalSession, HistoryCursor, Presence, SessionFolder},
};
use std::{
    cmp::Reverse,
    collections::HashMap,
    convert::Infallible,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

mod common;
pub(crate) use common::{
    Agent, Memos, Seen, first_line, is_jsonl, newest_files, open_database, quiet_presence,
    unix_millis, unix_secs, unix_seconds, wal_path, walk_files,
};
pub(crate) mod agy;
pub(crate) mod codex;
mod codex_run;
mod delete;
mod disk;
pub(crate) mod flsh;
pub(crate) mod memo;
pub(crate) mod opencode;
pub(crate) mod pi;
pub(crate) mod folder_place;
mod transcript;

pub(crate) use delete::{remove_dir_in, remove_file_in};
pub(crate) use disk::{forget_checkouts, rescan, saved_checkouts, walk};
pub(crate) use crate::fs::git_read::{
    Checkout, checkout, common_dir, default_branch, git_dir, last_commit, place, repository,
};
pub(crate) use memo::Memo;

/// The most distinct folders looked up with git in one listing.
const MAX_LOOKUPS: usize = 400;
/// Where Codex, pi, flsh, OpenCode and Antigravity keep their sessions on
/// this machine (Claude Code's are in `crate::logs::claude`). A session's folder counts
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
pub(crate) fn in_wsl_homes<T: Clone>(
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

/// Beep's own hidden folder in a home, which holds no sessions of the agents.
pub(crate) const HIDDEN: &str = ".beep";

/// The stores of the agents in each WSL distribution, kept by its home so
/// their memos last.
fn wsl_stores() -> Vec<(crate::machine::wsl::DistroName, Stores)> {
    static KEPT: std::sync::LazyLock<std::sync::Mutex<HashMap<PathBuf, Stores>>> =
        std::sync::LazyLock::new(Default::default);

    in_wsl_homes(&KEPT, |home| Some(Stores::in_wsl_home(home)))
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
    pub(crate) fn codex_children(&self, parent: &SessionId) -> Vec<crate::logs::subagent::Run> {
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
    pub(crate) fn codex_run(&self, threads: &[String]) -> Option<crate::logs::subagent::Run> {
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

    pub(crate) fn seen(&self, agent: Option<Agent>, id: &SessionId) -> Option<Seen> {
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
/// reading what they spent (`crate::logs::spend`).
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
    let hidden = home.map(|home| home.join(HIDDEN));
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

/// `folders` and, for each that resolves to another path (a symlink, or a
/// worktree start that came back canonical), that real path too, so a
/// session's `cwd` matches whichever form it was recorded in. Read once per
/// query, not once per session.
pub(crate) fn with_real(folders: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(folders.len());
    for folder in folders {
        let real = crate::fs::paths::canonicalize(&folder).ok();
        for one in [Some(folder), real].into_iter().flatten() {
            if !out.contains(&one) {
                out.push(one);
            }
        }
    }

    out
}

/// Which session folders lie in `folders`: as recorded, or, only when that
/// misses, with their links resolved, so a match is never lost to a symlink on
/// either side. Each distinct folder is resolved once. Reads the disk: run it
/// off the async workers.
pub(crate) struct Under<'a> {
    folders: &'a [PathBuf],
    resolved: HashMap<PathBuf, PathBuf>,
}

impl<'a> Under<'a> {
    pub(crate) fn new(folders: &'a [PathBuf]) -> Self {
        Self { folders, resolved: HashMap::new() }
    }

    pub(crate) fn has(&mut self, cwd: &Path) -> bool {
        let folders = self.folders;
        if folders.iter().any(|folder| within(cwd, folder)) {
            return true;
        }

        let real = self.resolved.entry(cwd.to_owned()).or_insert_with(|| {
            crate::fs::paths::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned())
        });

        folders.iter().any(|folder| within(real, folder))
    }
}

/// Whether `cwd` is `folder` or inside it.
pub(crate) fn within(cwd: &Path, folder: &Path) -> bool {
    folder.parent().is_some() && cwd.starts_with(folder)
}

/// The sessions of `seen` in any of `folders`, newest first, at most
/// `limit`.
pub(crate) fn in_folders(mut seen: Vec<Seen>, folders: &[PathBuf], limit: usize) -> Vec<Seen> {
    let mut under = Under::new(folders);
    seen.retain(|one| under.has(&one.cwd));
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
    let mut under = Under::new(folders);
    seen.retain(|one| {
        under.has(&one.cwd)
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

#[cfg(test)]
#[path = "../../tests/logs_found.rs"]
mod tests;
