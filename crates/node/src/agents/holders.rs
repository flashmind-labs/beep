//! Whether a session started outside Beep still runs: a Codex process
//! keeps its session's rollout open while it has the session loaded, so a
//! process holding the file is the sign, not when the file was written.

use crate::files::paths;
use beep_proto::node::agent::Presence;
#[cfg(target_os = "linux")]
use std::collections::HashSet;
use std::{collections::HashMap, path::PathBuf};

/// A process holding each of `files`, all looked up at once; `None`
/// when that cannot be told here.
pub(super) async fn held_open(files: Vec<PathBuf>) -> Option<HashMap<PathBuf, u32>> {
    if files.is_empty() {
        return Some(HashMap::new());
    }
    // By the path the system reports, to the path asked about.
    let real: HashMap<PathBuf, PathBuf> = files
        .into_iter()
        .filter_map(|file| Some((paths::canonicalize(&file).ok()?, file)))
        .collect();
    let open = open_among(&real).await?;
    Some(
        real.into_iter()
            .filter_map(|(real, file)| Some((file, *open.get(&real)?)))
            .collect(),
    )
}

/// The paths among `real`'s keys some process has open, read from `/proc`
/// in one pass.
#[cfg(target_os = "linux")]
async fn open_among(real: &HashMap<PathBuf, PathBuf>) -> Option<HashMap<PathBuf, u32>> {
    /// The most processes looked at.
    const MAX_PROCESSES: usize = 8192;
    let wanted: HashSet<PathBuf> = real.keys().cloned().collect();
    let scan = move || {
        let processes = std::fs::read_dir("/proc").ok()?;
        let mut open = HashMap::new();

        for process in processes
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()))
            })
            .take(MAX_PROCESSES)
        {
            // Another user's processes cannot be read, and do not count.
            let Ok(fds) = std::fs::read_dir(process.path().join("fd")) else {
                continue;
            };

            for fd in fds.flatten() {
                if let Ok(to) = std::fs::read_link(fd.path())
                    && wanted.contains(&to)
                    && let Some(pid) = process
                        .file_name()
                        .to_str()
                        .and_then(|pid| pid.parse().ok())
                {
                    open.entry(to).or_insert(pid);
                }
            }

            if open.len() == wanted.len() {
                break;
            }
        }

        Some(open)
    };
    tokio::task::spawn_blocking(scan).await.ok()?
}

/// The paths among `real`'s keys some process has open, through one `lsof`
/// given `LIMIT`.
#[cfg(all(unix, not(target_os = "linux")))]
async fn open_among(real: &HashMap<PathBuf, PathBuf>) -> Option<HashMap<PathBuf, u32>> {
    use std::{process::Stdio, time::Duration};
    use tokio::{process::Command, time::timeout};
    /// How long `lsof` may take.
    const LIMIT: Duration = Duration::from_secs(10);
    crate::run::process::counted("lsof".as_ref());
    let mut lsof = Command::new("lsof");
    lsof.args(["-F", "pn", "-w", "--"])
        .args(real.keys())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let out = timeout(LIMIT, lsof.output()).await.ok()?.ok()?;
    // lsof exits 1 when any file is not open; what it printed still counts.
    let mut open = HashMap::new();
    let mut pid = None;

    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = value.parse::<u32>().ok();
        } else if let Some(path) = line.strip_prefix('n')
            && let Some(pid) = pid
        {
            open.entry(PathBuf::from(path)).or_insert(pid);
        }
    }

    Some(open)
}

#[cfg(not(unix))]
async fn open_among(_real: &HashMap<PathBuf, PathBuf>) -> Option<HashMap<PathBuf, u32>> {
    None
}

/// What a session found in the agents' logs is doing, once what only its
/// log said is checked: `presence` as listed, `pid` the process seen
/// running it, and `held` whether a process has its file open, when
/// known. A recent write alone does not make a session live when nothing
/// holds it.
pub(super) fn settle(presence: Presence, pid: Option<u32>, held: Option<bool>) -> Presence {
    if pid.is_some() {
        return presence;
    }

    match held {
        // Quiet for a while, it may still be loaded in an app server.
        Some(true) if presence.is_live() => presence,
        Some(true) => Presence::Running,
        Some(false) if presence.is_live() => Presence::Stopped,
        _ => presence,
    }
}

#[cfg(test)]
#[path = "../tests/agents_holders.rs"]
mod tests;
