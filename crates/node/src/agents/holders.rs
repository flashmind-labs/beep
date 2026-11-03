//! Whether a session started outside Beep still runs: a Codex process
//! keeps its session's rollout open while it has the session loaded, so a
//! process holding the file is the sign, not when the file was written.

use beep_proto::node::agent::Presence;
use std::path::Path;

/// Whether some process has `file` open; `None` when that cannot be told
/// here. Blocks: run it with `spawn_blocking`.
#[cfg(target_os = "linux")]
pub(super) fn held_open(file: &Path) -> Option<bool> {
    /// The most processes looked at.
    const MAX_PROCESSES: usize = 8192;
    let file = std::fs::canonicalize(file).ok()?;
    let processes = std::fs::read_dir("/proc").ok()?;
    let held = processes
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()))
        })
        .take(MAX_PROCESSES)
        .any(|process| {
            // Another user's processes cannot be read, and do not count.
            let Ok(fds) = std::fs::read_dir(process.path().join("fd")) else {
                return false;
            };
            fds.flatten()
                .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|to| to == file))
        });
    Some(held)
}

/// Whether some process has `file` open, through `lsof`; `None` when that
/// cannot be told here. Blocks: run it with `spawn_blocking`.
#[cfg(all(unix, not(target_os = "linux")))]
pub(super) fn held_open(file: &Path) -> Option<bool> {
    use std::process::{Command, Stdio};
    let out = Command::new("lsof")
        .args(["-t", "-w", "--"])
        .arg(file)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    // lsof exits 1 with nothing printed when nothing has it open.
    Some(out.status.success() && !out.stdout.is_empty())
}

#[cfg(not(unix))]
pub(super) fn held_open(_file: &Path) -> Option<bool> {
    None
}

/// What a session found in the agents' logs is doing, once what only its
/// log said is checked: `presence` as listed, `pid` the process seen
/// running it, `beep_started` when this node started it (so it ran here
/// and ended with Beep or the node), and `held` whether a process has its
/// file open, when known. A recent write alone does not make a session
/// live when nothing holds it or Beep ran it.
pub(super) fn settle(
    presence: Presence,
    pid: Option<u32>,
    beep_started: bool,
    held: Option<bool>,
) -> Presence {
    if pid.is_some() {
        return presence;
    }
    match held {
        // Quiet for a while, it may still be loaded in an app server.
        Some(true) if presence.is_live() => presence,
        Some(true) => Presence::Running,
        Some(false) if presence.is_live() => Presence::Stopped,
        None if beep_started && presence == Presence::Recent => Presence::Stopped,
        _ => presence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recent_write_with_nothing_holding_it_is_stopped() {
        assert_eq!(
            settle(Presence::Recent, None, false, Some(false)),
            Presence::Stopped
        );
        assert_eq!(
            settle(Presence::Recent, None, false, Some(true)),
            Presence::Recent
        );
    }

    #[test]
    fn a_session_beep_ran_is_not_live_on_its_log_alone() {
        assert_eq!(
            settle(Presence::Recent, None, true, None),
            Presence::Stopped
        );
        // Without that, a recent write still counts when nothing can say.
        assert_eq!(
            settle(Presence::Recent, None, false, None),
            Presence::Recent
        );
    }

    #[test]
    fn a_process_seen_running_it_stays_live() {
        assert_eq!(
            settle(Presence::Working, Some(41), true, Some(false)),
            Presence::Working
        );
        // Quiet, but still loaded somewhere.
        assert_eq!(
            settle(Presence::Stopped, None, false, Some(true)),
            Presence::Running
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_this_process_holds_is_held() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let open = std::fs::File::open(&path).unwrap();
        assert_eq!(held_open(&path), Some(true));
        drop(open);
        assert_eq!(held_open(&path), Some(false));
    }
}
