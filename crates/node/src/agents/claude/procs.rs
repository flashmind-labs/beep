//! The Claude Code processes running on the machine, the folders they run
//! in and the sessions their arguments name, read with `ps`, then `lsof` on
//! macOS or `/proc` on Linux.

use super::discover::is_session_id;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::timeout};

/// How long `ps` or `lsof` may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The most processes looked at.
const MAX_PROCESSES: usize = 256;
/// The most words a program's path may hold: one that runs from a folder
/// with spaces in its name is split by `ps` like its arguments.
const MAX_PROGRAM_WORDS: usize = 8;
/// Claude Code's commands that run beside a session, not one themselves:
/// its daemon, the pty host it keeps a background session in, and the
/// spare it holds ready.
const HELPERS: [&str; 3] = ["daemon", "bg-pty-host", "bg-spare"];

/// A running `claude`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Process {
    pub(super) pid: u32,
    /// Its working folder, when it could be read.
    pub(super) cwd: Option<PathBuf>,
    /// The session its arguments name, the sure way to tie it to a
    /// transcript when it wrote no pid file (a session the daemon runs in
    /// the background writes none).
    pub(super) session: Option<String>,
}

/// Every running `claude` with its folder. An error reads as none: the
/// sessions then show as stopped or recently active.
pub(super) async fn claude_processes() -> Vec<Process> {
    let Some(listing) = output("ps", &["-axwwo", "pid=,args="]).await else {
        return Vec::new();
    };
    let found = claude_sessions(&listing, |program| Path::new(program).is_file());
    let pids: Vec<u32> = found.iter().map(|(pid, _)| *pid).collect();
    let cwds = folders(&pids).await;
    found
        .into_iter()
        .map(|(pid, session)| Process {
            pid,
            cwd: cwds
                .iter()
                .find(|(of, _)| *of == pid)
                .map(|(_, cwd)| cwd.clone()),
            session,
        })
        .collect()
}

/// The pids in `ps -o pid=,args=` output that run Claude Code, each with
/// the session its arguments name. Claude Code is the `claude` program, or
/// the binary its native install keeps under `claude/versions/`, which its
/// daemon runs by path; a `claude` running one of its `HELPERS` is not a
/// session. `is_file` says whether a path with spaces in it is a file here.
fn claude_sessions(listing: &str, is_file: impl Fn(&str) -> bool) -> Vec<(u32, Option<String>)> {
    listing
        .lines()
        .filter_map(|line| {
            let (pid, args) = line.trim().split_once(char::is_whitespace)?;
            let pid = pid.parse().ok()?;
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
fn session_named(rest: &str) -> Option<String> {
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
fn session_in(word: &str) -> Option<String> {
    let id = Path::new(word)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(word);
    is_session_id(id).then(|| id.to_owned())
}

#[cfg(target_os = "linux")]
async fn folders(pids: &[u32]) -> Vec<(u32, PathBuf)> {
    pids.iter()
        .filter_map(|pid| {
            let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
            Some((*pid, cwd))
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
async fn folders(pids: &[u32]) -> Vec<(u32, PathBuf)> {
    if pids.is_empty() {
        return Vec::new();
    }
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    match output("lsof", &["-a", "-d", "cwd", "-Fpn", "-p", &list]).await {
        Some(text) => lsof_folders(&text),
        None => Vec::new(),
    }
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

/// What `program` prints, `None` when it fails to run in time. `lsof`
/// exits 1 when one of the processes is gone, so its output still counts.
async fn output(program: &str, args: &[&str]) -> Option<String> {
    let run = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = timeout(PROBE_TIMEOUT, run).await.ok()?.ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
#[path = "../../tests/agents_claude_procs.rs"]
mod tests;
