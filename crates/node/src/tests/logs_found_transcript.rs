use super::*;
use serde_json::json;
use std::fs;

#[test]
fn a_flsh_session_reads_its_prompts_replies_and_calls() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let path = dir.path().join("session.jsonl");
    fs::write(&path, include_str!("../logs/found/fixtures/flsh.jsonl")).expect("a session file");
    let read: Vec<(EntryKind, String)> = flsh(&path)
        .expect("the session reads")
        .into_iter()
        .map(|entry| (entry.kind, entry.text))
        .collect();
    assert_eq!(
        read,
        [
            (EntryKind::User, "Run the tests in /srv/laptop/app".into()),
            (EntryKind::Thought, "The user wants the tests run.".into()),
            (EntryKind::Agent, "Running them.".into()),
            (EntryKind::Tool, "exec: cargo test".into()),
            (EntryKind::Agent, "All tests pass.".into()),
        ]
    );
}

/// The user entries the file of `lines` gives `read`.
fn users(lines: &[Value], read: fn(&Path) -> io::Result<Vec<Entry>>) -> Vec<String> {
    let dir = tempfile::tempdir().expect("a temp folder");
    let path = dir.path().join("session.jsonl");
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    fs::write(&path, text.join("\n") + "\n").expect("a session file");
    read(&path)
        .expect("the session reads")
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::User)
        .map(|entry| entry.text)
        .collect()
}

#[test]
fn a_pi_prompt_keeps_the_brief_apart() {
    let brief = crate::tags::tests::BARE_BRIEF;
    let line = json!({"type": "message", "message": {"role": "user", "content": [
        {"type": "text", "text": brief},
        {"type": "text", "text": "ok, let's keep going"}
    ]}});
    let shown = users(&[line], pi);
    assert_eq!(shown.len(), 1);
    assert_eq!(
        tags::parts(&shown[0]),
        [
            tags::Part::Context(brief),
            tags::Part::User("ok, let's keep going")
        ]
    );
}

#[test]
fn a_codex_prompt_keeps_the_brief_apart() {
    let brief = crate::tags::tests::BARE_BRIEF;
    let glued = json!({"type": "event_msg", "payload": {"type": "user_message",
        "message": format!("{brief}\nok, let's keep going")}});
    let item = json!({"type": "event_msg", "payload": {"type": "item_completed",
    "item": {"type": "UserMessage", "content": [
        {"type": "text", "text": brief},
        {"type": "text", "text": "and then?"}
    ]}}});
    let shown = users(&[glued, item], codex);
    let said: Vec<String> = shown.iter().map(|text| tags::said(text)).collect();
    assert_eq!(said, ["ok, let's keep going", "and then?"]);
}

#[test]
fn saved_flsh_delegations_show_as_agent_cards() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let path = dir.path().join("session.jsonl");
    let line = json!({"type": "assistant", "message": {"content": "", "toolCalls": [{
        "id": "call-1", "name": "delegate", "arguments": {
            "task": "Check files", "name": "worker", "model": "provider:model"
        }
    }]}});
    fs::write(&path, format!("{line}\n")).expect("a session file");
    let entries = flsh(&path).expect("the session reads");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].text.contains("[steps] 0"));
    assert!(entries[0].text.contains("[agent] worker"));
    assert!(entries[0].text.contains("[model] provider:model"));
}

#[test]
fn saved_flsh_and_pi_beep_calls_use_beep_titles() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let flsh_path = dir.path().join("flsh.jsonl");
    let flsh_line = json!({"type": "assistant", "message": {"content": "", "toolCalls": [{
        "id": "call-1", "name": "mcp__beep__start_agent",
        "arguments": {"harness": "pi", "machine": "build-box", "prompt": "Fix the tests"}
    }]}});
    fs::write(&flsh_path, format!("{flsh_line}\n")).expect("a flsh file");
    let flsh_entries = flsh(&flsh_path).expect("flsh reads");
    assert_eq!(
        flsh_entries[0].text,
        "Beep: Start pi agent on build-box: Fix the tests"
    );

    let pi_path = dir.path().join("pi.jsonl");
    let pi_line = json!({"type": "message", "message": {"role": "assistant", "content": [{
        "type": "toolCall", "id": "call-1", "name": "mcp__beep__remote_run",
        "arguments": {"machine": "build-box", "command": "cargo test"}
    }]}});
    fs::write(&pi_path, format!("{pi_line}\n")).expect("a pi file");
    let pi_entries = pi(&pi_path).expect("pi reads");
    assert_eq!(pi_entries[0].text, "Beep: Run on build-box: cargo test");
}

#[test]
fn saved_pi_subagent_calls_show_as_agent_cards() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let path = dir.path().join("session.jsonl");
    let line = json!({"type": "message", "message": {"role": "assistant", "content": [
        {"type": "toolCall", "id": "call-1", "name": "subagent",
         "arguments": {"agent": "scout", "task": "Find the config"}},
        {"type": "toolCall", "id": "call-2", "name": "subagent",
         "arguments": {"action": "status", "id": "run-1"}}
    ]}});
    fs::write(&path, format!("{line}\n")).expect("a session file");
    let entries = pi(&path).expect("the session reads");
    assert_eq!(entries.len(), 2);
    assert!(
        entries[0].text.contains("[agent] scout"),
        "{}",
        entries[0].text
    );
    assert_eq!(entries[1].kind, EntryKind::Tool);
}

#[test]
fn saved_pi_provider_errors_appear_in_transcript_and_custom_messages_stay_hidden() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let path = dir.path().join("session.jsonl");
    let lines = [
        json!({"type": "message", "message": {
            "role": "assistant",
            "content": [{"type": "toolCall", "id": "call-1", "name": "bash", "arguments": {"command": "echo hi"}}],
            "stopReason": "error",
            "errorMessage": "Provider finish_reason: content_filter"
        }}),
        json!({"type": "custom_message", "customType": "beep-error-recovery", "content": "hidden", "display": false}),
        json!({"type": "message", "message": {
            "role": "assistant",
            "content": [],
            "stopReason": "error",
            "errorMessage": "Upstream 502"
        }}),
    ];
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    fs::write(&path, text.join("\n") + "\n").expect("a session file");
    let entries: Vec<(EntryKind, String)> = pi(&path)
        .expect("the session reads")
        .into_iter()
        .map(|entry| (entry.kind, entry.text))
        .collect();
    assert_eq!(
        entries,
        [
            (EntryKind::Tool, "bash: echo hi".into()),
            (
                EntryKind::Other,
                "Provider finish_reason: content_filter".into()
            ),
            (EntryKind::Other, "Upstream 502".into()),
        ]
    );
}
