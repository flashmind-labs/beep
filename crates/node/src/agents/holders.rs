//! Whether a session started outside Beep still runs: a Codex process
//! keeps its session's rollout open while it has the session loaded, so a
//! process holding the file is the sign, not when the file was written.

use crate::files::paths;
use beep_proto::node::agent::Presence;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// Which of `files` some process has open, all looked up at once; `None`
/// when that cannot be told here.
pub(super) async fn held_open(files: Vec<PathBuf>) -> Option<HashSet<PathBuf>> {
    if files.is_empty() {
        return Some(HashSet::new());
    }
    // By the path the system reports, to the path asked about.
    let real: HashMap<PathBuf, PathBuf> = files
        .into_iter()
        .filter_map(|file| Some((paths::canonicalize(&file).ok()?, file)))
        .collect();
    let open = open_among(&real).await?;
    Some(
        real.into_iter()
            .filter(|(real, _)| open.contains(real))
            .map(|(_, file)| file)
            .collect(),
    )
}

/// The paths among `real`'s keys some process has open, read from `/proc`
/// in one pass.
#[cfg(target_os = "linux")]
async fn open_among(real: &HashMap<PathBuf, PathBuf>) -> Option<HashSet<PathBuf>> {
    /// The most processes looked at.
    const MAX_PROCESSES: usize = 8192;
    let wanted: HashSet<PathBuf> = real.keys().cloned().collect();
    let scan = move || {
        let processes = std::fs::read_dir("/proc").ok()?;
        let mut open = HashSet::new();
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
                {
                    open.insert(to);
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
async fn open_among(real: &HashMap<PathBuf, PathBuf>) -> Option<HashSet<PathBuf>> {
    use std::{process::Stdio, time::Duration};
    use tokio::{process::Command, time::timeout};
    /// How long `lsof` may take.
    const LIMIT: Duration = Duration::from_secs(10);
    let lsof = Command::new("lsof")
        .args(["-F", "n", "-w", "--"])
        .args(real.keys())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = timeout(LIMIT, lsof).await.ok()?.ok()?;
    // lsof exits 1 when any file is not open; what it printed still counts.
    let open = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .map(PathBuf::from)
        .collect();
    Some(open)
}

#[cfg(not(unix))]
async fn open_among(_real: &HashMap<PathBuf, PathBuf>) -> Option<HashSet<PathBuf>> {
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
mod tests {
    use super::*;

    #[test]
    fn a_recent_write_with_nothing_holding_it_is_stopped() {
        assert_eq!(
            settle(Presence::Recent, None, Some(false)),
            Presence::Stopped
        );
        assert_eq!(settle(Presence::Recent, None, Some(true)), Presence::Recent);
        // When nothing can say, a recent write still counts.
        assert_eq!(settle(Presence::Recent, None, None), Presence::Recent);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_file_this_process_holds_is_held() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let other = dir.path().join("other.jsonl");
        std::fs::write(&other, "{}\n").unwrap();
        let _open = std::fs::File::open(&path).unwrap();
        // A path nothing opened beside it. (A file closed again may still be
        // held for a moment by a child another test forks before it execs.)
        let held = held_open(vec![path.clone(), other]).await;
        assert_eq!(held, Some(HashSet::from([path])));
    }
}
