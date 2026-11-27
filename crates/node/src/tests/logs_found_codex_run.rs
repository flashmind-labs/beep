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
        json!({"type": "turn_context", "payload": {"model": "parent-model"}}),
        json!({"type": "event_msg", "payload": {"type": "task_started"}}),
        json!({"type": "turn_context", "payload": {"model": "child-model"}}),
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
            session: Some(SessionId::from(format!("subagent:codex:{THREAD}"))),
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
            model: Some("child-model".into()),
            task: Some("sleep_test".into()),
        }
    );
    let running = read(&write(root.path(), false)).expect("the rollout");
    assert!(running.working);
    assert_eq!(running.result, None);
}

#[test]
fn task_path_children_are_linked_only_to_their_parent_and_refresh_independently() {
    let root = tempfile::tempdir().expect("a temp folder");
    write(root.path(), false);
    let parent = root
        .path()
        .join("sessions/2026/11/03/rollout-parent-p1.jsonl");
    fs::write(&parent, format!("{}\n", json!({"type": "session_meta", "payload": {
        "id": "p1", "source": "cli", "cwd": "/srv/project", "timestamp": "2026-11-03T07:00:00Z"
    }}))).expect("a parent rollout");
    let stores = crate::logs::found::Stores {
        codex: Some(root.path().to_owned()),
        ..Default::default()
    };
    assert!(
        stores
            .codex_children(&SessionId::from("unrelated"))
            .is_empty()
    );
    assert!(
        stores.codex_children(&SessionId::from(THREAD)).is_empty(),
        "no grandchildren here"
    );
    let children = stores.codex_children(&SessionId::from("p1"));
    assert_eq!(children.len(), 1);
    assert!(children[0].working);
    assert_eq!(children[0].task.as_deref(), Some("sleep_test"));
    let before = stores
        .transcript(&SessionId::from("p1"), 10, None)
        .expect("read")
        .expect("parent");
    assert_eq!(before.entries.len(), 1);
    assert!(
        before.entries[0]
            .text
            .contains("sleep_test [in_progress]\n[steps] 2")
    );
    assert!(
        before.entries[0]
            .text
            .contains(&format!("[session] subagent:codex:{THREAD}"))
    );

    write(root.path(), true);
    let after = stores
        .transcript(&SessionId::from("p1"), 10, None)
        .expect("read")
        .expect("parent");
    assert_eq!(after.entries.len(), 1, "the child is not duplicated");
    assert!(after.entries[0].text.contains("sleep_test [completed]"));
    assert!(after.entries[0].text.contains("Slept for a minute."));
}

#[test]
fn child_headers_reject_self_links_and_invalid_thread_ids() {
    for thread in ["p1", "../child"] {
        let line = json!({"type": "session_meta", "payload": {"id": thread,
            "source": {"subagent": {"thread_spawn": {"parent_thread_id": "p1"}}}}});
        assert!(parent(&line.to_string()).is_none());
    }
}
