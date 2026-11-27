//! The machine's process table, read once for every reader, and the signals
//! that stop what it lists.

use std::{collections::HashMap, sync::Arc, time::Duration};

mod process;
mod snapshot;

pub(crate) use process::Process;
pub(crate) use snapshot::{invalidate, read};

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) mod linux;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod macos;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) use linux as os;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use macos as os;
#[cfg(windows)]
pub(crate) use windows as os;

/// Sends SIGTERM, or SIGKILL when `kill`, to `p` unless it is gone or its pid
/// now belongs to another process.
#[cfg(unix)]
pub(crate) fn signal(p: &Process, kill: bool) -> std::io::Result<()> {
    if !os::alive(p) {
        return Ok(());
    }

    let pid = libc::pid_t::try_from(p.pid).map_err(std::io::Error::other)?;
    // SAFETY: the positive pid and signal are plain integers. Its birth was
    // checked above, so a stale observation is not sent to a reused pid.
    if unsafe { libc::kill(pid, if kill { libc::SIGKILL } else { libc::SIGTERM }) } != 0 {
        let e = std::io::Error::last_os_error();

        if e.raw_os_error() != Some(libc::ESRCH) {
            return Err(e);
        }
    }

    Ok(())
}
#[cfg(windows)]
pub(crate) use windows::signal;

/// How often a session reads the process table while it works or a client
/// follows `processes`: a wrapper that daemonizes and exits between reads
/// would lose its child, and closing the session would leave it running.
pub(crate) const WATCHED: Duration = Duration::from_secs(1);

/// How often an idle session reads the process table, only to keep track of children that
/// leave the session's group, so closing the session can stop them. Idle
/// sessions share a table up to this old, so however many there are, the
/// OS is read about once per interval.
pub(crate) const UNWATCHED: Duration = Duration::from_secs(60);

/// The machine's processes, by pid, from the table every reader shares: it
/// is read at most once a second however many ask.
pub(crate) async fn table() -> anyhow::Result<Arc<HashMap<u32, Process>>> {
    read(WATCHED).await
}

