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
