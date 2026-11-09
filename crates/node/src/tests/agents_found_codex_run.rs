use super::*;
use serde_json::json;
use std::fs;

const THREAD: &str = "01a1a004-0000-7000-8000-000000000001";

/// A fabricated rollout of a forked subagent: two lines of its parent's
/// history, then its own turn.
fn rollout_lines(done: bool) -> String {
    let mut lines = vec![
        json!({"type": "session_meta", "payload": {"id": THREAD, "cwd": "/srv/project",
            "source": {"subagent": {"thread_spawn": {"parent_thread_id": "p1", "depth": 1,
                "agent_path": "/root/sleep_test", "agent_nickname": "Kepler",
                "agent_role": null}}},
            "agent_nickname": "Kepler", "agent_path": "/root/sleep_test",
            "subagent_history_start_ordinal": 3}}),
        json!({"type": "response_item", "payload": {"type": "function_call",
            "name": "exec_command", "arguments": "{\"cmd\":\"ls\"}", "call_id": "old"}}),
        json!({"type": "response_item", "payload": {"type": "message", "role": "assistant",
            "content": [{"type": "output_text", "text": "the parent's answer"}]}}),
        json!({"type": "event_msg", "payload": {"type": "task_started"}}),
        json!({"type": "response_item", "payload": {"type": "function_call",
            "name": "sleep", "arguments": "{\"duration_ms\":60000}", "call_id": "c1"}}),
        json!({"type": "response_item", "payload": {"type": "function_call_output",
            "call_id": "c1", "output": "Sleep completed."}}),
        json!({"type": "response_item", "payload": {"type": "custom_tool_call",
            "name": "exec", "input": "\nrun('cargo test')\n", "call_id": "c2"}}),
    ];

    if done {
        lines.push(
            json!({"type": "event_msg", "payload": {"type": "task_complete",
            "last_agent_message": "Slept for a minute."}}),
        );
    }

    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn write(root: &Path, done: bool) -> PathBuf {
    let dir = root.join("sessions/2026/11/03");
    fs::create_dir_all(&dir).expect("folders");
    let path = dir.join(format!("rollout-2026-11-03T07-07-01-{THREAD}.jsonl"));
    fs::write(&path, rollout_lines(done)).expect("a rollout");
    path
}

#[test]
fn a_subagents_rollout_is_found_by_its_thread() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = write(root.path(), true);
    assert_eq!(rollout(root.path(), THREAD), Some(path));
    assert_eq!(rollout(root.path(), "other"), None);
    assert_eq!(rollout(root.path(), "../x"), None);
}

#[test]
fn its_own_steps_and_answer_read_after_the_parents_history() {
    let root = tempfile::tempdir().expect("a temp folder");
    let run = read(&write(root.path(), true)).expect("the rollout");
    assert_eq!(
        run,
        SubagentRun {
            session: Some(format!("subagent:codex:{THREAD}")),
            steps: vec![
                RunStep {
                    title: "sleep".into(),
                    done: true
                },
                RunStep {
                    title: "exec: run('cargo test')".into(),
                    done: false
                },
            ],
            count: 2,
            result: Some("Slept for a minute.".into()),
            working: false,
            name: Some("Kepler".into()),
            task: Some("sleep_test".into()),
        }
    );
    let running = read(&write(root.path(), false)).expect("the rollout");
    assert!(running.working);
    assert_eq!(running.result, None);
}
