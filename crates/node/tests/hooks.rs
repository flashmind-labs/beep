//! `beep-node hook`, the command Claude Code's hooks run, against a node
//! listening in this process: it delivers what Beep queued for a session
//! the way Claude Code's hook contract reads it, and passes through at
//! once when no node listens.
#![cfg(unix)]

use beep_node::claude::External;
use beep_proto::node::agent::{LiveState, Steer};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

const SESSION: &str = "0f3c2a9e-1111-4222-8333-944455556666";
/// Well past the hook's own one second, well under Claude Code's two.
const QUICK: Duration = Duration::from_millis(1500);

/// Runs the hook for `event` with `fields`; returns what it printed.
async fn hook(dir: &Path, event: &str, fields: Value) -> String {
    let mut input = json!({
        "session_id": SESSION,
        "transcript_path": "/srv/none.jsonl",
        "cwd": "/srv/project",
        "hook_event_name": event,
    });
    if let (Some(input), Some(fields)) = (input.as_object_mut(), fields.as_object()) {
        input.extend(fields.clone());
    }
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .arg("--dir")
        .arg(dir)
        .arg("hook")
        .env("HOME", dir)
        .env("CLAUDE_CONFIG_DIR", dir.join("claude"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the hook starts");
    let mut stdin = child.stdin.take().expect("stdin is piped");
    stdin
        .write_all(input.to_string().as_bytes())
        .await
        .expect("the hook reads its input");
    drop(stdin);
    let out = child.wait_with_output().await.expect("the hook ends");
    assert!(out.status.success(), "{event}: {:?}", out.status);
    assert!(
        started.elapsed() < QUICK,
        "{event} took {:?}",
        started.elapsed()
    );
    String::from_utf8(out.stdout).expect("the hook prints UTF-8")
}

fn bash(command: &str) -> Value {
    json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "tool_use_id": "toolu_01",
        "tool_response": { "stdout": "ok" },
    })
}

fn decision(printed: &str) -> Value {
    serde_json::from_str(printed).unwrap_or_else(|e| panic!("{e}: {printed:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn hooks_deliver_what_beep_queued_and_pass_through_without_a_node() {
    // No node listens: the hook prints nothing and exits at once.
    let down = TempDir::new().unwrap();
    assert_eq!(
        hook(down.path(), "PreToolUse", bash("cargo test")).await,
        ""
    );
    assert_eq!(hook(down.path(), "Stop", json!({})).await, "");

    let dir = TempDir::new().unwrap();
    let external = Arc::new(External::default());
    let listening = {
        let external = external.clone();
        let dir = dir.path().to_owned();
        tokio::spawn(async move { external.listen_hooks(&dir).await })
    };
    timeout(Duration::from_secs(10), async {
        while !dir.path().join("hooks.sock").exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the node listens");
    let live = |steer| external.live(SESSION, steer, HashMap::new());

    // Before any hook, nothing can be queued.
    assert!(live(Some(Steer::Stop {})).await.is_err());

    assert_eq!(hook(dir.path(), "PreToolUse", bash("cargo test")).await, "");
    let status = live(None).await.unwrap();
    assert_eq!(status.state, LiveState::Working);
    let tool = status.tool.expect("a tool runs");
    assert_eq!(
        (tool.name.as_str(), tool.detail.as_str()),
        ("Bash", "cargo test")
    );

    // A message waits for the tool call to end, then blocks with it once.
    let text = "use the staging database";
    live(Some(Steer::Message { text: text.into() }))
        .await
        .unwrap();
    assert_eq!(hook(dir.path(), "PreToolUse", bash("ls")).await, "");
    let blocked = decision(&hook(dir.path(), "PostToolUse", bash("ls")).await);
    assert_eq!(blocked["decision"], "block");
    assert!(
        blocked["reason"].as_str().unwrap().contains(text),
        "{blocked}"
    );
    assert_eq!(hook(dir.path(), "PostToolUse", bash("ls")).await, "");

    // A stop denies each tool call until the turn ends.
    live(Some(Steer::Stop {})).await.unwrap();
    for _ in 0..2 {
        let denied = decision(&hook(dir.path(), "PreToolUse", bash("rm -r build")).await);
        let output = &denied["hookSpecificOutput"];
        assert_eq!(output["hookEventName"], "PreToolUse");
        assert_eq!(output["permissionDecision"], "deny");
        assert_eq!(
            output["permissionDecisionReason"],
            "The user asked you to stop."
        );
    }
    let last = json!({ "last_assistant_message": "Stopped.", "stop_hook_active": false });
    assert_eq!(hook(dir.path(), "Stop", last.clone()).await, "");
    let status = live(None).await.unwrap();
    assert_eq!(status.state, LiveState::Idle);
    assert_eq!(status.session.last_message, "Stopped.");

    // Idle at its prompt, it cannot be steered.
    let idle = live(Some(Steer::Message { text: text.into() })).await;
    assert!(
        idle.as_ref()
            .is_err_and(|e| e.to_string().contains("idle at its prompt")),
        "{idle:?}"
    );

    // A message queued while it thinks keeps it from ending the turn.
    assert_eq!(
        hook(dir.path(), "UserPromptSubmit", json!({ "prompt": "go" })).await,
        ""
    );
    live(Some(Steer::Message { text: text.into() }))
        .await
        .unwrap();
    let kept = decision(&hook(dir.path(), "Stop", last).await);
    assert_eq!(kept["decision"], "block");
    assert!(kept["reason"].as_str().unwrap().contains(text), "{kept}");
    assert_eq!(live(None).await.unwrap().state, LiveState::Working);

    listening.abort();
}
