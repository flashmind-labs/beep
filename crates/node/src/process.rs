//! Children that lead their own process group, so stopping one stops
//! everything it started: the shell's commands, an agent's tools.

use std::{
    os::fd::{AsFd, OwnedFd},
    time::Duration,
};
use tokio::{process::Child, time::timeout};

/// How long a child has to exit after SIGTERM before the group is killed.
pub(crate) const STOP_GRACE: Duration = Duration::from_secs(2);

/// A child that leads its own process group (spawned with
/// `process_group(0)`). `stop` ends the group gently, then hard; dropping it
/// without `stop` kills the group at once, since `Drop` cannot wait.
pub(crate) struct GroupLeader {
    child: Child,
    /// The group's id, which is the child's pid.
    group: Option<libc::pid_t>,
}

impl GroupLeader {
    pub(crate) fn new(child: Child) -> Self {
        let group = child.id().and_then(|pid| libc::pid_t::try_from(pid).ok());
        Self { child, group }
    }

    pub(crate) fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Copies of the read ends of the child's stdout and stderr. Held while
    /// it stops, they keep a write on its way out landing in the pipe after
    /// the readers are gone, rather than killing it with SIGPIPE.
    pub(crate) fn hold_output(&self) -> Vec<OwnedFd> {
        let stdout = self.child.stdout.as_ref().map(AsFd::as_fd);
        let stderr = self.child.stderr.as_ref().map(AsFd::as_fd);
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

    /// Sends SIGTERM to the group, waits up to `grace` for the child to
    /// exit, then SIGKILLs whatever is left of the group and reaps the
    /// child, so no zombie remains.
    pub(crate) async fn stop(&mut self, grace: Duration) {
        if self.reaped() {
            return;
        }
        self.signal(libc::SIGTERM);
        if timeout(grace, self.child.wait()).await.is_err() {
            log::debug!("the process group outlived SIGTERM; killing it");
            self.signal(libc::SIGKILL);
            if let Err(e) = self.child.wait().await {
                log::warn!("could not reap a stopped child: {e}");
            }
            return;
        }
        // The child exited, but something it started may still run. POSIX
        // does not reuse a process group's id while the group has members,
        // so this reaches what is left of the group or no one.
        if let Some(group) = self.group {
            killpg(group, libc::SIGKILL);
        }
    }

    /// The child was waited for, so its pid may name another process now.
    fn reaped(&self) -> bool {
        self.child.id().is_none()
    }

    /// Sends `signal` to the group while its leader is not yet reaped: until
    /// then the leader, a zombie at worst, keeps the id from being reused.
    fn signal(&self, signal: libc::c_int) {
        if let Some(group) = self.group
            && !self.reaped()
        {
            killpg(group, signal);
        }
    }
}

impl Drop for GroupLeader {
    fn drop(&mut self) {
        // Only a signal, so it is fine here. After `stop` this sends nothing.
        self.signal(libc::SIGKILL);
    }
}

fn killpg(group: libc::pid_t, signal: libc::c_int) {
    // SAFETY: killpg takes two integers and only sends a signal; it reads
    // and writes no memory of this process.
    let result = unsafe { libc::killpg(group, signal) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            log::warn!("could not signal process group {group}: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::process::ExitStatusExt,
        process::{ExitStatus, Stdio},
    };
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, BufReader},
        process::{ChildStdout, Command},
    };

    /// Starts `script` in its own group and waits for its first line, so
    /// its traps are set before it is signalled.
    async fn start(script: &str) -> (GroupLeader, BufReader<ChildStdout>) {
        let mut child = Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .expect("sh starts");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .await
            .expect("the script writes");
        assert_eq!(line, "ready\n");
        (GroupLeader::new(child), stdout)
    }

    /// How the reaped child ended; tokio keeps it after `wait`.
    fn exit(leader: &mut GroupLeader) -> ExitStatus {
        leader
            .child
            .try_wait()
            .expect("try_wait works")
            .expect("the child was reaped")
    }

    /// Reads to the end, which comes only once every process holding
    /// stdout, the whole group, is gone. The timeout bounds a failure.
    async fn wait_closed(stdout: &mut BufReader<ChildStdout>) -> String {
        let mut rest = String::new();
        timeout(Duration::from_secs(20), stdout.read_to_string(&mut rest))
            .await
            .expect("the whole group stopped")
            .expect("stdout reads");
        rest
    }

    #[tokio::test]
    async fn a_child_that_takes_sigterm_exits_cleanly() {
        let (mut leader, mut stdout) =
            start("trap 'echo terminated; exit 0' TERM; echo ready; while :; do sleep 1; done")
                .await;
        // A grace far longer than the test runs: SIGTERM alone ends it.
        leader.stop(Duration::from_secs(600)).await;
        assert!(leader.reaped());
        assert_eq!(exit(&mut leader).code(), Some(0), "it exited on its own");
        assert!(wait_closed(&mut stdout).await.contains("terminated"));
    }

    #[tokio::test]
    async fn a_child_that_ignores_sigterm_is_killed_after_the_grace() {
        let (mut leader, mut stdout) =
            start("trap 'echo ignored' TERM; echo ready; while :; do sleep 1; done").await;
        leader.stop(Duration::from_millis(200)).await;
        assert!(leader.reaped());
        assert_eq!(exit(&mut leader).signal(), Some(libc::SIGKILL));
        wait_closed(&mut stdout).await;
    }

    #[tokio::test]
    async fn dropping_a_group_leader_stops_what_it_started() {
        // The background sleep keeps stdout open: it reads to its end only
        // once the sleep is gone as well as the shell.
        let (leader, mut stdout) = start("sleep 300 & echo ready; wait").await;
        drop(leader);
        wait_closed(&mut stdout).await;
    }
}
