//! Process groups and signals.

use std::{
    io,
    os::fd::{AsFd, OwnedFd},
};
use tokio::process::{Child, Command};

pub(crate) type Held = OwnedFd;

/// A process group, by its id: the leader's pid. Not `Copy`, like the
/// Windows job it stands in for.
#[derive(Debug, Clone)]
pub(crate) struct Group(libc::pid_t);

impl Group {
    pub(super) fn of(child: &Child) -> io::Result<Option<Self>> {
        Ok(child
            .id()
            .and_then(|pid| libc::pid_t::try_from(pid).ok())
            .map(Self))
    }

    /// The group a session leader leads, such as a pseudo-terminal's
    /// child, by the leader's pid.
    pub(crate) fn led_by(pid: u32) -> Option<Self> {
        libc::pid_t::try_from(pid).ok().map(Self)
    }

    /// Tells the group its terminal hung up, with SIGHUP: what ends an
    /// interactive shell, which ignores SIGTERM.
    pub(crate) fn hang_up(&self) {
        killpg(self.0, libc::SIGHUP);
    }

    /// Asks the group to end, with SIGTERM.
    pub(crate) fn terminate(&self) {
        killpg(self.0, libc::SIGTERM);
    }

    /// Ends the group now, with SIGKILL.
    pub(crate) fn kill(&self) {
        killpg(self.0, libc::SIGKILL);
    }
}

pub(super) fn own_group(command: &mut Command) -> &mut Command {
    command.process_group(0)
}

pub(super) fn hold_output(child: &Child) -> Vec<Held> {
    let stdout = child.stdout.as_ref().map(AsFd::as_fd);
    let stderr = child.stderr.as_ref().map(AsFd::as_fd);
    stdout
        .into_iter()
        .chain(stderr)
        .filter_map(|fd| match fd.try_clone_to_owned() {
            Ok(held) => Some(held),
            Err(e) => {
                log::debug!("could not hold a pipe of the child: {e}");
                None
            }
        })
        .collect()
}

fn killpg(group: libc::pid_t, signal: libc::c_int) {
    // SAFETY: killpg takes two integers and only sends a signal; it reads
    // and writes no memory of this process.
    let result = unsafe { libc::killpg(group, signal) };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            log::warn!("could not signal process group {group}: {error}");
        }
    }
}
