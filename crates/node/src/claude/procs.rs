//! The Claude Code processes running on the machine and the folders they
//! run in, read with `ps`, then `lsof` on macOS or `/proc` on Linux.

use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

/// How long `ps` or `lsof` may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The most processes looked at.
const MAX_PROCESSES: usize = 256;

/// A running `claude`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Process {
    pub(super) pid: u32,
    /// Its working folder, when it could be read.
    pub(super) cwd: Option<PathBuf>,
}

/// Every running `claude` with its folder. An error reads as none: the
/// sessions then show as stopped or recently active.
pub(super) async fn claude_processes() -> Vec<Process> {
    let Some(listing) = output("ps", &["-axo", "pid=,comm="]).await else {
        return Vec::new();
    };
    let pids = claude_pids(&listing);
    let cwds = folders(&pids).await;
    pids.into_iter()
        .map(|pid| Process {
            pid,
            cwd: cwds
                .iter()
                .find(|(of, _)| *of == pid)
                .map(|(_, cwd)| cwd.clone()),
        })
        .collect()
}

/// The pids in `ps -o pid=,comm=` output whose program is `claude`.
fn claude_pids(listing: &str) -> Vec<u32> {
    listing
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(char::is_whitespace)?;
            let program = command.trim().rsplit('/').next()?;
            if program != "claude" {
                return None;
            }
            pid.parse().ok()
        })
        .take(MAX_PROCESSES)
        .collect()
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
mod tests {
    use super::*;

    #[test]
    fn lsof_output_pairs_pids_with_folders() {
        let text = "p101\nfcwd\nn/srv/project\np103\nfcwd\nn/srv/other dir\n";
        assert_eq!(
            lsof_folders(text),
            [
                (101, PathBuf::from("/srv/project")),
                (103, PathBuf::from("/srv/other dir"))
            ]
        );
    }
}
