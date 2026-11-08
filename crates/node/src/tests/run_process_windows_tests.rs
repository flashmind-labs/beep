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
