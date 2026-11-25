use super::*;
use serde_json::json;

fn cursor(start: u64, next: u64) -> SessionCursor {
    SessionCursor {
        start,
        next,
        fingerprint: next.to_string(),
    }
}

fn page(earlier: u64, texts: &[&str]) -> Value {
    let entries: Vec<Value> = texts
        .iter()
        .map(|text| json!({"kind": "assistant", "text": text}))
        .collect();
    json!({"earlier": earlier, "entries": entries, "turns": earlier})
}

fn texts(held: &Held) -> Vec<&str> {
    held.page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap())
        .collect()
}

#[test]
fn updates_join_as_the_webview_joins_them() {
    let held = join(None, false, page(0, &["a", "b"]), &cursor(0, 2)).expect("a snapshot");

    // A delta that rewrites the last entry and adds one; the newest fields win.
    let held = join(Some(held), true, page(1, &["B", "c"]), &cursor(0, 3)).expect("a delta");
    assert_eq!(texts(&held), ["a", "B", "c"]);
    assert_eq!(held.page["turns"], 1);
    assert_eq!(held.cursor.next, 3);

    // The node moves its window on: the page starts at the cursor.
    let held = join(Some(held), true, page(3, &["d"]), &cursor(2, 4)).expect("a delta");
    assert_eq!(texts(&held), ["c", "d"]);
    assert_eq!(held.page["earlier"], 2);

    // A gap, or a delta with nothing to join, is dropped.
    assert!(join(Some(held), true, page(6, &["g"]), &cursor(2, 7)).is_none());
    assert!(join(None, true, page(0, &["a"]), &cursor(0, 1)).is_none());
    // A page that does not end at its cursor is dropped.
    assert!(join(None, false, page(0, &["a"]), &cursor(0, 2)).is_none());
}

fn pushed(kind: UpdateKind, earlier: u32, texts: &[&str], start: u64) -> Pushed {
    use beep_proto::{AgentEntry, AgentEntryKind, AgentStatus, MachineSession};
    use std::time::SystemTime;

    let entries: Vec<_> = texts
        .iter()
        .map(|text| AgentEntry {
            kind: AgentEntryKind::Agent {},
            text: (*text).to_owned(),
            tool: None,
            sent_at: None,
        })
        .collect();
    let next = u64::from(earlier) + entries.len() as u64;
    Pushed {
        kind,
        cursor: cursor(start, next),
        resumed: false,
        view: AgentSessionView {
            session: MachineSession {
                reply: None,
                changes: None,
                machine_id: "build-box".into(),
                machine_name: "build-box".into(),
                id: "one".into(),
                agent: "codex".into(),
                title: None,
                cwd: "/srv/project".into(),
                status: AgentStatus::Working {},
                doing: None,
                started_at: SystemTime::UNIX_EPOCH,
                last_activity: SystemTime::UNIX_EPOCH,
                external: false,
                question: None,
                subagents: 0,
                model: None,
                assistant: false,
                account: None,
                persona: None,
                place: None,
                continues: None,
            },
            plan: Vec::new(),
            files: Vec::new(),
            last_message: String::new(),
            error: None,
            queued: 0,
            turns: 0,
            entries,
            earlier,
            queued_messages: Vec::new(),
            model: None,
            last_stop: None,
            usage: None,
        },
        modes: None,
        tasks: Vec::new(),
    }
}

#[tokio::test(start_paused = true)]
async fn a_running_session_is_joined_typed_and_written_when_due() {
    let dir = tempfile::tempdir().unwrap();
    let kept = Arc::new(KeptTranscripts::new(dir.path().to_owned()));
    let key = SessionKey {
        machine_id: "build-box".into(),
        agent: "codex".into(),
        session: "one".into(),
    };
    let mut keeper = Keeper::new(kept.clone(), key.clone());
    let start = Instant::now();

    keeper
        .take(&pushed(UpdateKind::Snapshot, 0, &["a"], 0), start)
        .await;
    assert_eq!(keeper.due(), None, "the first page is written at once");

    for (at, text) in ["b", "c", "d"].into_iter().enumerate() {
        let delta = pushed(UpdateKind::Delta, 1 + at as u32, &[text], 1);
        keeper.take(&delta, start + Duration::from_secs(1)).await;
    }
    assert_eq!(keeper.due(), Some(start + RUNNING_EVERY));

    keeper.write(start + RUNNING_EVERY);
    assert_eq!(keeper.due(), None);

    let page = kept.read(key).await.unwrap().expect("kept").page;
    let texts: Vec<_> = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["b", "c", "d"]);
    assert_eq!(page["earlier"], 1);
}
