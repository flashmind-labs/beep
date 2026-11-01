//! Children that lead their own process group, so stopping one stops
//! everything it started: the shell's commands, an agent's tools. On Unix
//! the group is a process group; on Windows it is a job object.

use std::{ffi::OsStr, time::Duration};
use tokio::{
    process::{Child, Command},
    time::timeout,
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

pub(crate) use imp::{Group, Held};

/// How long a child has to exit after SIGTERM before the group is killed.
pub(crate) const STOP_GRACE: Duration = Duration::from_secs(2);

/// `program` as a command to start. Windows starts only `.exe` files by
/// bare name, so there it is looked up on `PATH` with every extension
/// `PATHEXT` lists: npm installs tools such as `npx` and `claude` as `.cmd`
/// files.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let program = program.as_ref();
    #[cfg(windows)]
    if let Ok(found) = which::which(program) {
        return Command::new(found);
    }
    Command::new(program)
}

/// Makes `command` start in a group of its own, for `GroupLeader`.
pub(crate) fn own_group(command: &mut Command) -> &mut Command {
    imp::own_group(command)
}

/// A child that leads its own group (spawned after `own_group`). `stop`
/// ends the group gently, then hard; dropping it without `stop` kills the
/// group at once, since `Drop` cannot wait.
pub(crate) struct GroupLeader {
    child: Child,
    group: Option<Group>,
}

impl GroupLeader {
    pub(crate) fn new(child: Child) -> Self {
        let group = match Group::of(&child) {
            Ok(group) => group,
            Err(e) => {
                log::warn!("could not group a child's processes: {e}");
                None
            }
        };
        Self { child, group }
    }

    /// The group, to kill it from elsewhere.
    pub(crate) fn group(&self) -> Option<Group> {
        self.group.clone()
    }

    pub(crate) fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Copies of the read ends of the child's stdout and stderr. Held while
    /// it stops, they keep a write on its way out landing in the pipe after
    /// the readers are gone, rather than killing it with SIGPIPE.
    pub(crate) fn hold_output(&self) -> Vec<Held> {
        imp::hold_output(&self.child)
    }

    /// Asks the group to end, waits up to `grace` for the child to exit,
    /// then kills whatever is left of the group and reaps the child, so no
    /// zombie remains. Windows has no gentle way: the group is killed at
    /// once.
    pub(crate) async fn stop(&mut self, grace: Duration) {
        if self.reaped() {
            return;
        }
        self.signal(Group::terminate);
        if timeout(grace, self.child.wait()).await.is_err() {
            log::debug!("the process group outlived SIGTERM; killing it");
            self.signal(Group::kill);
            if let Err(e) = self.child.wait().await {
                log::warn!("could not reap a stopped child: {e}");
            }
            return;
        }
        // The child exited, but something it started may still run. POSIX
        // does not reuse a process group's id while the group has members,
        // so this reaches what is left of the group or no one.
        if let Some(group) = &self.group {
            group.kill();
        }
    }

    /// The child was waited for, so its pid may name another process now.
    fn reaped(&self) -> bool {
        self.child.id().is_none()
    }

    /// Signals the group while its leader is not yet reaped: until then the
    /// leader, a zombie at worst, keeps the id from being reused.
    fn signal(&self, send: fn(&Group)) {
        if let Some(group) = &self.group
            && !self.reaped()
        {
            send(group);
        }
    }
}

impl Drop for GroupLeader {
    fn drop(&mut self) {
        // Only a signal, so it is fine here. After `stop` this sends nothing.
        self.signal(Group::kill);
    }
}

#[cfg(all(test, unix))]
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

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    #[tokio::test]
    async fn stopping_a_leader_ends_what_it_started() {
        // ping inherits stdout, so it reads to its end only once ping is
        // gone as well as cmd.
        let mut command = Command::new("cmd");
        command
            .args(["/d", "/c", "echo ready& ping -n 300 127.0.0.1"])
            .stdout(Stdio::piped());
        let mut child = own_group(&mut command).spawn().expect("cmd starts");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut leader = GroupLeader::new(child);
        assert!(leader.group().is_some(), "the child is in a job");
        let mut line = String::new();
        stdout.read_line(&mut line).await.expect("cmd writes");
        assert_eq!(line.trim_end(), "ready");
        leader.stop(Duration::from_secs(600)).await;
        assert!(leader.reaped());
        let mut rest = Vec::new();
        timeout(Duration::from_secs(20), stdout.read_to_end(&mut rest))
            .await
            .expect("the whole job stopped")
            .expect("stdout reads");
    }
}
