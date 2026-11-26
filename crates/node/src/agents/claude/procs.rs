//! The Claude Code processes running on the machine, the folders they run
//! in and the sessions their arguments name, read from the node's shared
//! process table, then `lsof` on macOS or `/proc` on Linux.

use beep_proto::SessionId;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

/// The most processes looked at.
const MAX_PROCESSES: usize = 256;
/// The most words a program's path may hold: one that runs from a folder
/// with spaces in its name is split by `ps` like its arguments.
const MAX_PROGRAM_WORDS: usize = 8;
/// Claude Code's commands that run beside a session, not one themselves:
/// its daemon, the pty host it keeps a background session in, and the
/// spare it holds ready.
const HELPERS: [&str; 3] = ["daemon", "bg-pty-host", "bg-spare"];

/// Claude Code's session ids are UUIDs; anything else is not looked up,
/// so an id never walks out of the store.
pub(super) fn is_session_id(id: &str) -> bool {
    id.len() == 36
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && id.matches('-').count() == 4
}

/// A running `claude`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Process {
    pub(super) pid: u32,
    /// Its working folder, when it could be read.
    pub(super) cwd: Option<PathBuf>,
    /// The session its arguments name, the sure way to tie it to a
    /// transcript when it wrote no pid file (a session the daemon runs in
    /// the background writes none).
    pub(super) session: Option<SessionId>,
}

/// Every running `claude` with its folder, from the process table the
/// node shares; a folder is looked up once per process. An error reads as
/// none: the sessions then show as stopped or recently active.
pub(super) async fn claude_processes() -> Vec<Process> {
    let table = match crate::agents::processes::table().await {
        Ok(table) => table,
        Err(e) => {
            log::debug!("could not list processes: {e:#}");
            return Vec::new();
        }
    };
    let mut listed: Vec<_> = table.values().collect();
    listed.sort_by_key(|process| process.pid);
    let found = claude_sessions(
        listed
            .iter()
            .map(|process| (process.pid, process.command.as_ref())),
        |program| Path::new(program).is_file(),
    );
    let running: Vec<(u32, u64)> = found
        .iter()
        .filter_map(|(pid, _)| Some((*pid, table.get(pid)?.birth)))
        .collect();
    let mut cwds = kept_folders(&running).await;
    found
        .into_iter()
        .map(|(pid, session)| Process {
            pid,
            cwd: cwds.remove(&pid),
            session,
        })
        .collect()
}

/// The folders of `running`, each a pid with its creation time, read for
/// a process not seen before and kept while it runs: a session's folder
/// does not change.
async fn kept_folders(running: &[(u32, u64)]) -> HashMap<u32, PathBuf> {
    let unknown: Vec<u32> = {
        let mut kept = crate::lock(&FOLDERS);
        kept.retain(|pid, folder| running.contains(&(*pid, folder.birth)));
        running
            .iter()
            .filter(|(pid, _)| !kept.contains_key(pid))
            .map(|(pid, _)| *pid)
            .collect()
    };

    // A failed read keeps nothing, so the next one asks again.
    if !unknown.is_empty()
        && let Some(read) = folders(&unknown).await
    {
        let found: HashMap<u32, PathBuf> = read.into_iter().collect();
        let mut kept = crate::lock(&FOLDERS);

        for (pid, birth) in running.iter().filter(|(pid, _)| unknown.contains(pid)) {
            kept.insert(
                *pid,
                Folder {
                    birth: *birth,
                    cwd: found.get(pid).cloned(),
                },
            );
        }
    }

    crate::lock(&FOLDERS)
        .iter()
        .filter_map(|(pid, folder)| Some((*pid, folder.cwd.clone()?)))
        .collect()
}

/// The folders of the Claude Code processes seen, by pid.
static FOLDERS: LazyLock<Mutex<HashMap<u32, Folder>>> = LazyLock::new(Mutex::default);

/// A Claude Code process's folder, `None` when it could not be read, with
/// the process's creation time, so a reused pid is not given another's.
struct Folder {
    birth: u64,
    cwd: Option<PathBuf>,
}

/// The pids among `processes`, each a pid with its command line, that run
/// Claude Code, each with the session its arguments name. Claude Code is
/// the `claude` program, or the binary its native install keeps under
/// `claude/versions/`, which its daemon runs by path; a `claude` running one of its `HELPERS` is not a
/// session. `is_file` says whether a path with spaces in it is a file here.
fn claude_sessions<'a>(
    processes: impl IntoIterator<Item = (u32, &'a str)>,
    is_file: impl Fn(&str) -> bool,
) -> Vec<(u32, Option<SessionId>)> {
    processes
        .into_iter()
        .filter_map(|(pid, args)| {
            let rest = claude_arguments(args.trim(), &is_file)?;

            if rest
                .split_whitespace()
                .next()
                .is_some_and(|first| HELPERS.contains(&first))
            {
                return None;
            }

            Some((pid, session_named(rest)))
        })
        .take(MAX_PROCESSES)
        .collect()
}

/// What follows the program in `args` when the program is Claude Code. The
/// program may be a path with spaces in it, so each prefix of `args` that
/// ends at a space is tried; one past the first word counts only as a file
/// that is here, so that `git -C /src/claude status` is not Claude Code.
fn claude_arguments<'a>(args: &'a str, is_file: &impl Fn(&str) -> bool) -> Option<&'a str> {
    let ends = args
        .match_indices(' ')
        .map(|(at, _)| at)
        .chain(std::iter::once(args.len()));
    for (word, end) in ends.take(MAX_PROGRAM_WORDS).enumerate() {
        let program = &args[..end];

        if is_claude(program) && (word == 0 || is_file(program)) {
            return Some(args[end..].trim_start());
        }
    }

    None
}

/// Whether `program` is Claude Code: `claude`, by name or by path, or the
/// versioned binary of its native install, `<dir>/claude/versions/<version>`.
fn is_claude(program: &str) -> bool {
    let path = Path::new(program);
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if name == "claude" {
        return true;
    }

    let called = |path: Option<&Path>, name: &str| {
        path.and_then(Path::file_name)
            .is_some_and(|dir| dir == name)
    };
    let versions = path.parent();
    let claude = versions.and_then(Path::parent);
    is_version(name) && called(versions, "versions") && called(claude, "claude")
}

/// Whether `name` is a version such as `2.1.311` or `2.1.311-beta`.
fn is_version(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_digit())
        && name.contains('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// The session the arguments `rest` name: `--session-id <id>`, else the
/// session `--resume` (or `-r`) names, as an id or the path of its
/// transcript, unless `--fork-session` makes that a new session.
fn session_named(rest: &str) -> Option<SessionId> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    // The flag's value: the session id in the words up to the next flag,
    // as a transcript's path may hold spaces.
    let value = |flag: &str| {
        let at = words.iter().position(|word| *word == flag)?;
        words[at + 1..]
            .iter()
            .take_while(|word| !word.starts_with('-'))
            .find_map(|word| session_in(word))
    };
    let value_or_equals = |flag: &str| {
        words
            .iter()
            .find_map(|word| session_in(word.strip_prefix(flag)?.strip_prefix('=')?))
            .or_else(|| value(flag))
    };

    match value_or_equals("--session-id") {
        Some(id) => Some(id),
        None if words.contains(&"--fork-session") => None,
        None => value_or_equals("--resume").or_else(|| value("-r")),
    }
}

/// The session id `word` is, or ends in as a transcript's path.
fn session_in(word: &str) -> Option<SessionId> {
    let id = Path::new(word)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(word);
    is_session_id(id).then(|| Into::<SessionId>::into(id))
}

/// The folders `pids` run in; `None` when they could not be read.
#[cfg(target_os = "linux")]
async fn folders(pids: &[u32]) -> Option<Vec<(u32, PathBuf)>> {
    Some(
        pids.iter()
            .filter_map(|pid| {
                let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
                Some((*pid, cwd))
            })
            .collect(),
    )
}

/// The folders `pids` run in; `None` when they could not be read.
#[cfg(not(target_os = "linux"))]
async fn folders(pids: &[u32]) -> Option<Vec<(u32, PathBuf)>> {
    if pids.is_empty() {
        return Some(Vec::new());
    }

    use std::{process::Stdio, time::Duration};
    use tokio::{process::Command, time::timeout};
    /// How long `lsof` may take.
    const LIMIT: Duration = Duration::from_secs(5);
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    crate::run::process::counted("lsof".as_ref());
    let run = Command::new("lsof")
        .args(["-a", "-d", "cwd", "-Fpn", "-p", &list])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    // lsof exits 1 when one of the processes is gone; its output still counts.
    let out = timeout(LIMIT, run).await.ok()?.ok()?;
    Some(lsof_folders(&String::from_utf8_lossy(&out.stdout)))
}

/// The folders in `lsof -Fpn` output: a `p<pid>` line, then `n<path>`.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn lsof_folders(text: &str) -> Vec<(u32, PathBuf)> {
    let mut found = Vec::new();
    let mut pid = None;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('p') {
            pid = rest.parse().ok();
        } else if let Some(path) = line.strip_prefix('n')
            && let Some(pid) = pid
        {
            found.push((pid, PathBuf::from(path)));
        }
    }

    found
}

#[cfg(test)]
#[path = "../../tests/agents_claude_procs.rs"]
mod tests;
