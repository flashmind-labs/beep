//! The git checkouts coding agents worked in, from the sessions Claude
//! Code, Codex, pi, flsh and OpenCode keep on this machine, for suggesting
//! projects to import and for listing an imported project's sessions.
//!
//! Each store is read on its own (`codex`, `pi`, `flsh`, `opencode`; Claude Code's in
//! `crate::claude`) into `Seen` sessions: the agent, its id, the folder it
//! ran in and when. Folders are then folded into the checkout that holds
//! them (`git`), so a session in a subfolder or a worktree counts for the
//! checkout's main folder. Every read is capped, and all of it is blocking
//! work for `spawn_blocking`. What a file said is kept (`memo`) until the
//! file changes, so listing again reads only what changed.

use crate::agents::cut;
use beep_proto::{
    AgentSlug,
    node::agent::{AgentSessions, ExternalSession, Presence, SessionFolder},
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

mod codex;
mod flsh;
mod git;
mod memo;
mod opencode;
mod pi;

pub(crate) use git::{Checkout, checkout};
pub(crate) use memo::Memo;

/// The most session files one store looks at.
const MAX_FILES: usize = 2_000;
/// The most distinct folders looked up with git in one listing.
const MAX_LOOKUPS: usize = 400;
/// The longest first line read from a session file.
const MAX_FIRST_LINE: u64 = 512 * 1024;
/// How much of a session file is read for its first prompt or its name.
const MAX_HEAD: u64 = 1024 * 1024;
/// The longest title or prompt kept.
const MAX_TITLE: usize = 300;
/// A session written this recently may still run.
const RECENT: Duration = Duration::from_secs(3 * 60);

/// A coding agent whose sessions are read here.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Agent {
    Claude,
    Codex,
    Pi,
    Flsh,
    OpenCode,
}

impl Agent {
    const ALL: [Self; 5] = [
        Self::Claude,
        Self::Codex,
        Self::Pi,
        Self::Flsh,
        Self::OpenCode,
    ];

    /// The name the node's agent table uses, such as `claude`.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Flsh => "flsh",
            Self::OpenCode => "opencode",
        }
    }

    /// The slug the wire names it by.
    pub(crate) fn slug(self) -> AgentSlug {
        AgentSlug::from(self.as_str())
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| agent.as_str() == name)
    }
}

/// One session as its store lists it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seen {
    pub(crate) agent: Agent,
    /// The agent's own id for it.
    pub(crate) id: String,
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

/// Where Codex, pi, flsh and OpenCode keep their sessions on this machine.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stores {
    codex: Option<PathBuf>,
    pi: Option<PathBuf>,
    flsh: Option<PathBuf>,
    /// OpenCode's database, or its data folder of them.
    opencode: Option<PathBuf>,
    memos: Arc<Memos>,
}

/// What the stores' files said, kept while they are unchanged.
#[derive(Debug)]
struct Memos {
    /// Each Codex or pi session file's header.
    heads: Memo<Option<Seen>>,
    /// Each Codex or pi session file's name or first prompt.
    titles: Memo<Option<String>>,
    /// Codex's index of the names the user gave.
    names: Memo<Arc<HashMap<String, String>>>,
    /// Each flsh or OpenCode database's sessions.
    databases: Memo<Arc<Vec<Seen>>>,
}

impl Default for Memos {
    fn default() -> Self {
        Self {
            heads: Memo::new(MAX_FILES * 4),
            titles: Memo::new(1_000),
            names: Memo::new(4),
            databases: Memo::new(1_000),
        }
    }
}

impl Stores {
    /// The stores that exist here.
    pub(crate) fn locate() -> Self {
        Self {
            codex: codex::locate(),
            pi: pi::locate(),
            flsh: flsh::locate(),
            opencode: opencode::locate(),
            memos: Arc::default(),
        }
    }

    /// Every session in the stores, `agent`'s only when given.
    pub(crate) fn sessions(&self, agent: Option<Agent>) -> Vec<Seen> {
        let wants = |one: Agent| agent.is_none_or(|agent| agent == one);
        let mut seen = Vec::new();
        let memos = &self.memos;
        if let Some(root) = self.codex.as_deref().filter(|_| wants(Agent::Codex)) {
            seen.extend(codex::sessions(root, memos));
        }
        if let Some(root) = self.pi.as_deref().filter(|_| wants(Agent::Pi)) {
            seen.extend(pi::sessions(root, memos));
        }
        if let Some(root) = self.flsh.as_deref().filter(|_| wants(Agent::Flsh)) {
            seen.extend(flsh::sessions(root, memos));
        }
        if let Some(root) = self.opencode.as_deref().filter(|_| wants(Agent::OpenCode)) {
            seen.extend(opencode::sessions(root, memos));
        }
        seen
    }

    /// `seen` as `describe` lists it, its title read from its file when the
    /// store gave none.
    pub(crate) fn describe(&self, mut seen: Seen, now: SystemTime) -> ExternalSession {
        if seen.title.is_none()
            && let Some(file) = seen.file.as_deref()
        {
            let read = || Ok::<_, Infallible>(title(seen.agent, file));
            let Ok(title) = self.memos.titles.read(file, read);
            seen.title = title;
        }
        describe(seen, now)
    }

    /// The file of session `id`, of `agent` when given; `None` for one not
    /// found or kept in a database.
    pub(crate) fn file(&self, agent: Option<Agent>, id: &str) -> Option<PathBuf> {
        self.sessions(agent)
            .into_iter()
            .find(|seen| seen.id == id)?
            .file
    }

    /// One session of `agent` by its id, described.
    pub(crate) fn find(&self, agent: Agent, id: &str) -> Option<ExternalSession> {
        let seen = self
            .sessions(Some(agent))
            .into_iter()
            .find(|seen| seen.id == id)?;
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

    #[cfg(test)]
    pub(crate) fn with_opencode(mut self, opencode: &Path) -> Self {
        self.opencode = Some(opencode.to_owned());
        self
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
}

impl LogRoots {
    pub(crate) fn locate() -> Self {
        Self {
            codex: codex::locate(),
            pi: pi::locate(),
            flsh: flsh::locate(),
            opencode: opencode::locate(),
        }
    }
}

/// OpenCode's databases at `root`: the file itself, or those in the folder.
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

pub(crate) fn opencode_databases(root: &Path) -> Vec<PathBuf> {
    opencode::databases_at(root)
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
/// `home` and the root, and folders that are gone or not in a checkout,
/// are left out. `lookup` finds a folder's checkout, as `checkout` does.
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
    let skipped = |path: &Path| path.parent().is_none() || Some(path) == home;
    let mut roots: HashMap<PathBuf, (String, Tally)> = HashMap::new();
    for (cwd, tally) in cwds.into_iter().take(MAX_LOOKUPS) {
        if skipped(cwd) || !cwd.is_absolute() || !cwd.is_dir() {
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
                        agent: agent.as_str().to_owned(),
                        sessions,
                    })
                    .collect(),
                last_activity: tally.last_activity,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.last_activity
            .cmp(&a.last_activity)
            .then_with(|| a.path.cmp(&b.path))
    });
    out.truncate(limit);
    out
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

/// The name or first prompt in `agent`'s session file at `file`.
fn title(agent: Agent, file: &Path) -> Option<String> {
    match agent {
        Agent::Codex => codex::title(file),
        Agent::Pi => pi::title(file),
        Agent::Claude | Agent::Flsh | Agent::OpenCode => None,
    }
}

/// `seen` as the server lists a session started outside Beep: its title
/// (`Stores::describe` reads one from the file), and whether it may still
/// run.
fn describe(seen: Seen, now: SystemTime) -> ExternalSession {
    let quiet = now
        .duration_since(UNIX_EPOCH + Duration::from_secs(seen.last_activity))
        .unwrap_or_default();
    ExternalSession {
        id: seen.id,
        agent: seen.agent.as_str().to_owned(),
        cwd: seen.cwd.to_string_lossy().into_owned(),
        title: seen.title,
        started_at: seen.started_at,
        last_activity: seen.last_activity,
        presence: if quiet < RECENT {
            Presence::Recent
        } else {
            Presence::Stopped
        },
        ..ExternalSession::default()
    }
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

/// The complete lines in the first `MAX_HEAD` bytes of `path`.
fn head_lines(path: &Path) -> io::Result<impl Iterator<Item = String>> {
    let reader = BufReader::new(File::open(path)?.take(MAX_HEAD));
    Ok(reader.lines().map_while(Result::ok))
}

/// A prompt as a title: trimmed, cut short, and none for text an agent
/// wraps in tags, such as its own context.
fn prompt_title(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && !text.starts_with('<')).then(|| cut(text, MAX_TITLE))
}

fn unix(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The newest `MAX_FILES` files under `dir` for which `wanted` holds,
/// `depth` folders down, newest first.
fn newest_files(dir: &Path, depth: usize, wanted: &dyn Fn(&Path) -> bool) -> Vec<(PathBuf, u64)> {
    let mut found = Vec::new();
    collect(dir, depth, wanted, &mut found);
    found.sort_by_key(|(_, at)| Reverse(*at));
    found.truncate(MAX_FILES);
    found
}

/// Gathers files, stopping well past `MAX_FILES` so a huge store stays
/// bounded.
fn collect(
    dir: &Path,
    depth: usize,
    wanted: &dyn Fn(&Path) -> bool,
    out: &mut Vec<(PathBuf, u64)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES * 10 {
            return;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if depth > 0 && kind.is_dir() {
            collect(&path, depth - 1, wanted, out);
        } else if depth == 0 && kind.is_file() && wanted(&path) {
            let at = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .map_or(0, unix);
            out.push((path, at));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(agent: Agent, id: &str, cwd: &Path, at: u64) -> Seen {
        Seen {
            agent,
            id: id.into(),
            cwd: cwd.to_owned(),
            started_at: at,
            last_activity: at,
            title: None,
            file: None,
        }
    }

    #[test]
    fn agents_read_their_names() {
        for agent in Agent::ALL {
            assert_eq!(Agent::from_name(agent.as_str()), Some(agent));
        }
        assert_eq!(Agent::from_name("opencode"), Some(Agent::OpenCode));
        assert_eq!(Agent::from_name("gemini"), None);
    }

    #[test]
    fn sessions_fold_into_their_checkout() {
        let root = tempfile::tempdir().expect("a temp folder");
        let home = root.path().join("home/user");
        let repo = home.join("proj");
        let sub = repo.join("crates/core");
        let tree = repo.join(".worktrees/fix");
        let loose = home.join("notes");
        for dir in [&sub, &tree, &loose] {
            std::fs::create_dir_all(dir).expect("folders");
        }
        let gone = home.join("gone");
        let all = vec![
            seen(Agent::Claude, "a", &repo, 100),
            seen(Agent::Claude, "b", &sub, 300),
            seen(Agent::Codex, "c", &tree, 200),
            seen(Agent::Pi, "d", &loose, 400),
            seen(Agent::Claude, "e", &home, 500),
            seen(Agent::Flsh, "f", Path::new("/"), 600),
            seen(Agent::Codex, "g", &gone, 700),
        ];
        let lookup = |dir: &Path| {
            dir.starts_with(&repo).then(|| Checkout {
                root: repo.clone(),
                origin: "git@example.com:org/proj.git".into(),
            })
        };
        let found = folders(&all, Some(&home), 10, lookup);
        assert_eq!(found.len(), 1, "{found:?}");
        let one = &found[0];
        assert_eq!(one.path, repo.to_string_lossy());
        assert_eq!(one.origin, "git@example.com:org/proj.git");
        assert_eq!(one.last_activity, 300);
        assert_eq!(
            one.agents,
            [
                AgentSessions {
                    agent: "claude".into(),
                    sessions: 2
                },
                AgentSessions {
                    agent: "codex".into(),
                    sessions: 1
                },
            ]
        );
    }

    #[test]
    fn a_checkout_at_home_is_left_out() {
        let root = tempfile::tempdir().expect("a temp folder");
        let home = root.path().to_owned();
        let all = vec![seen(Agent::Claude, "a", &home, 1)];
        let lookup = |_: &Path| {
            Some(Checkout {
                root: home.clone(),
                origin: String::new(),
            })
        };
        assert!(folders(&all, Some(&home), 10, lookup).is_empty());
    }

    #[test]
    fn a_project_lists_its_sessions_and_its_subfolders_newest_first() {
        let repo = PathBuf::from("/home/user/proj");
        let all = vec![
            seen(Agent::Claude, "a", &repo, 1),
            seen(Agent::Codex, "b", &repo.join("sub"), 3),
            seen(Agent::Pi, "c", Path::new("/home/user/proj-other"), 5),
        ];
        let picked = in_folders(all, &[repo], 10);
        let ids: Vec<&str> = picked.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn an_old_session_reads_as_stopped() {
        let now = SystemTime::now();
        let mut one = seen(Agent::Flsh, "s", Path::new("/home/user/proj"), unix(now));
        one.title = Some("Fix the build".into());
        let described = describe(one.clone(), now);
        assert_eq!(described.presence, Presence::Recent);
        assert_eq!(described.title.as_deref(), Some("Fix the build"));
        assert_eq!(described.agent, "flsh");
        one.last_activity = 0;
        assert_eq!(describe(one, now).presence, Presence::Stopped);
    }

    #[test]
    fn a_session_is_found_by_its_agent_and_id() {
        let root = tempfile::tempdir().expect("a temp folder");
        let dir = root.path().join("--home-user-proj--");
        std::fs::create_dir_all(&dir).expect("folders");
        std::fs::write(
            dir.join("2026-08-27T23-01-39-077Z_p1.jsonl"),
            "{\"type\":\"session\",\"version\":3,\"id\":\"p1\",\"timestamp\":\"2026-08-27T23:01:39.077Z\",\"cwd\":\"/home/user/proj\"}\n\
             {\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"Tidy up\"}}\n",
        )
        .expect("a session");
        let stores = Stores::at(None, Some(root.path()), None);
        let found = stores.find(Agent::Pi, "p1").expect("the session");
        assert_eq!(found.cwd, "/home/user/proj");
        assert_eq!(found.title.as_deref(), Some("Tidy up"));
        assert_eq!(found.presence, Presence::Recent, "just written");
        assert!(stores.find(Agent::Codex, "p1").is_none());
        assert!(stores.find(Agent::Pi, "p2").is_none());
    }

    /// Writes `text` over `path` and gives it back its modification time,
    /// so only a read of its content can tell it changed.
    fn rewrite_in_place(path: &Path, text: &str) {
        let modified = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .expect("a modification time");
        std::fs::write(path, text).expect("the rewrite");
        File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(modified))
            .expect("the old time back");
    }

    #[test]
    fn an_unchanged_session_file_is_not_read_again_and_a_changed_one_is() {
        let root = tempfile::tempdir().expect("a temp folder");
        let dir = root.path().join("--home-user-proj--");
        std::fs::create_dir_all(&dir).expect("folders");
        let path = dir.join("2026-08-27T23-01-39-077Z_p1.jsonl");
        let session = |cwd: &str, prompt: &str| {
            format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"p1\",\"timestamp\":\"2026-08-27T23:01:39.077Z\",\"cwd\":\"{cwd}\"}}\n\
                 {{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"{prompt}\"}}}}\n"
            )
        };
        std::fs::write(&path, session("/home/user/proj", "Tidy up")).expect("a session");
        let stores = Stores::at(None, Some(root.path()), None);
        let found = stores.find(Agent::Pi, "p1").expect("the session");
        assert_eq!(found.title.as_deref(), Some("Tidy up"));

        rewrite_in_place(&path, &session("/home/user/proi", "Tidy on"));
        let found = stores.find(Agent::Pi, "p1").expect("the session");
        assert_eq!(found.cwd, "/home/user/proj", "kept from the first read");
        assert_eq!(found.title.as_deref(), Some("Tidy up"));

        std::fs::write(&path, session("/home/user/other", "Tidy it all up")).expect("a session");
        let found = stores.find(Agent::Pi, "p1").expect("the session");
        assert_eq!(found.cwd, "/home/user/other");
        assert_eq!(found.title.as_deref(), Some("Tidy it all up"));
    }

    #[test]
    fn an_opencode_session_is_found_by_its_id() {
        let root = tempfile::tempdir().expect("a temp folder");
        opencode::tests::database(&root.path().join("opencode.db"));
        let stores = Stores::at(None, None, None).with_opencode(root.path());
        let found = stores.find(Agent::OpenCode, "ses_a").expect("the session");
        assert_eq!(found.agent, "opencode");
        assert_eq!(found.cwd, "/home/user/proj");
        assert_eq!(found.title.as_deref(), Some("Port the tests"));
        assert_eq!(found.presence, Presence::Stopped);
        assert!(
            stores.find(Agent::OpenCode, "ses_c").is_none(),
            "a subagent's"
        );
        assert!(stores.find(Agent::Flsh, "ses_a").is_none());
    }

    #[test]
    fn tagged_prompts_are_not_titles() {
        assert_eq!(prompt_title("  <environment_context>"), None);
        assert_eq!(prompt_title(" Fix it "), Some("Fix it".into()));
        assert_eq!(prompt_title(""), None);
    }
}
