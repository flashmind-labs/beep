//! A node told to stop by SIGTERM, as systemd does on a restart, says so at
//! once and exits well inside the service manager's limit (#340).
#![cfg(unix)]

use iroh::SecretKey;
use rustix::process::{Pid, Signal, kill_process};
use std::{fs, os::unix::fs::PermissionsExt, process::Stdio, time::Duration};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{ChildStderr, Command},
    time::timeout,
};

/// What the node logs once it serves the mesh commands.
const READY: &str = "taking mesh commands";
/// How long the node may take to start.
const START_LIMIT: Duration = Duration::from_secs(90);
/// How long a stop may take: under systemd's 20 s with room to spare.
const STOP_LIMIT: Duration = Duration::from_secs(15);

/// Reads lines until one contains `text`, keeping what was read.
async fn logged(lines: &mut Lines<BufReader<ChildStderr>>, log: &mut Vec<String>, text: &str) {
    while let Some(line) = lines.next_line().await.expect("stderr reads") {
        let found = line.contains(text);
        log.push(line);
        if found {
            return;
        }
    }
    panic!("the node ended before logging {text:?}; it logged {log:#?}");
}

#[tokio::test]
async fn sigterm_is_logged_and_the_node_exits_in_time() {
    let dir = TempDir::new().expect("a temp folder");
    let home = TempDir::new().expect("a temp folder");
    let key_file = dir.path().join("key");
    fs::write(&key_file, SecretKey::generate().to_bytes()).expect("the key is written");
    fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).expect("the key is private");
    // A random port, so the live service's is left alone.
    fs::write(dir.path().join("node.toml"), "name = \"box-a\"\nport = 0\n")
        .expect("settings are written");
    let mut child = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .arg("--dir")
        .arg(dir.path())
        .arg("run")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("BEEP_NODE_NO_RELAY", "1")
        .env("CLAUDE_CONFIG_DIR", home.path().join(".claude"))
        .env("CODEX_HOME", home.path().join(".codex"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the node starts");
    let stderr = child.stderr.take().expect("stderr is piped");
    let mut lines = BufReader::new(stderr).lines();
    let mut log = Vec::new();
    timeout(START_LIMIT, logged(&mut lines, &mut log, READY))
        .await
        .unwrap_or_else(|_| panic!("the node did not start in time; it logged {log:#?}"));

    let pid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
        .expect("the node has a pid");
    kill_process(pid, Signal::TERM).expect("SIGTERM is sent");

    let stopped = timeout(STOP_LIMIT, async {
        logged(&mut lines, &mut log, "stopping: SIGTERM").await;
        logged(&mut lines, &mut log, "stopped").await;
        child.wait().await.expect("the node is waited for")
    })
    .await
    .unwrap_or_else(|_| panic!("the node did not stop in time; it logged {log:#?}"));
    assert!(stopped.success(), "the node exited with {stopped}");
}
