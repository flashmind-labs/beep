use super::*;
use beep_proto::node::agent::{Entry, EntryKind};

#[tokio::test]
async fn event_sources_are_shared_and_stop_with_the_last_subscription() {
    let root = tempfile::tempdir().unwrap();
    let agents = Arc::new(Agents {
        stores: super::super::found::Stores::at(Some(root.path()), None, None),
        external: super::super::claude::External::locate_in(root.path()),
        ..Agents::default()
    });
    let first = agents.watch_event_sources().await.unwrap();
    let second = agents.watch_event_sources().await.unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    let weak = Arc::downgrade(&first);
    drop(first);
    assert!(weak.upgrade().is_some());
    drop(second);
    assert!(weak.upgrade().is_none());
    let restarted = agents.watch_event_sources().await.unwrap();
    assert!(!std::sync::Weak::ptr_eq(&weak, &Arc::downgrade(&restarted)));
}

#[cfg(unix)]
#[tokio::test]
async fn an_external_process_exit_publishes_agent_and_summary_events() {
    let root = tempfile::tempdir().unwrap();
    let agents = Arc::new(Agents {
        stores: super::super::found::Stores::at(Some(root.path()), None, None),
        external: super::super::claude::External::locate_in(root.path()),
        ..Agents::default()
    });
    let _lease = agents.watch_event_sources().await.unwrap();
    let mut changes = agents.events.subscribe();
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", "read message"])
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    agents.observe_external(&[beep_proto::node::agent::ExternalSession {
        id: "external-example".into(),
        agent: "codex".into(),
        cwd: root.path().to_string_lossy().into_owned(),
        pid: child.id(),
        ..Default::default()
    }]);
    assert_eq!(agents.observed_work_roots(), [root.path().to_owned()]);
    drop(child.stdin.take());
    timeout(Duration::from_secs(5), async {
        loop {
            let event = changes.recv().await.unwrap();

            if event.topic == beep_proto::node::events::Topic::keyed("agent", "external-example") {
                break;
            }
        }

        loop {
            let event = changes.recv().await.unwrap();

            if event.topic.name() == "agents" {
                break;
            }
        }
    })
    .await
    .unwrap();
    child.wait().await.unwrap();
    assert!(agents.observed_work_roots().is_empty());
}

#[tokio::test]
async fn source_summaries_ignore_transcript_appends_but_notice_title_changes() {
    use std::{fs::OpenOptions, io::Write};
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions/2026/11/01");
    std::fs::create_dir_all(&sessions).unwrap();
    let file = sessions.join("rollout-example.jsonl");
    std::fs::write(&file, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"example\",\"cwd\":\"/srv/project\",\"source\":\"cli\"}}\n").unwrap();
    let agents = Agents {
        stores: super::super::found::Stores::at(Some(root.path()), None, None),
        ..Agents::default()
    };
    let before = source_summary(&agents).await.unwrap();
    assert_eq!(before.len(), 1);
    let mut writer = OpenOptions::new().append(true).open(file).unwrap();
    writeln!(writer, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"output\"}}}}").unwrap();
    assert_eq!(source_summary(&agents).await.unwrap(), before);
    std::fs::write(
        root.path().join("session_index.jsonl"),
        "{\"id\":\"example\",\"thread_name\":\"Renamed session\"}\n",
    )
    .unwrap();
    assert_eq!(
        source_summary(&agents).await.unwrap()[0].title.as_deref(),
        Some("Renamed session")
    );
}

#[tokio::test]
async fn database_only_sessions_return_the_existing_transcript_error_and_refresh_metadata() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir(&folder).unwrap();
    let database = rusqlite::Connection::open(folder.join("sessions.db")).unwrap();
    database.execute_batch("PRAGMA journal_mode = WAL;
        CREATE TABLE session_meta (chat_key TEXT PRIMARY KEY, title TEXT, working_dir TEXT, created_at INTEGER);
        CREATE TABLE sessions (chat_key TEXT, entry_kind TEXT, content TEXT, turn_index INTEGER, created_at INTEGER);
        INSERT INTO session_meta VALUES ('example', 'Original', '/srv/project', 100);").unwrap();
    let agents = Arc::new(Agents {
        stores: super::super::found::Stores::at(None, None, Some(root.path())),
        ..Agents::default()
    });
    assert_eq!(
        source_summary(&agents).await.unwrap()[0].title.as_deref(),
        Some("Original")
    );
    let link = beep_proto::node::local_link();
    let task = tokio::spawn(serve_follow(link.node.0, link.node.1, agents.clone()));
    let (mut tx, mut rx) = link.caller;
    tx.send(&FollowSession {
        agent: "flsh".into(),
        session: "example".into(),
        last: 200,
        after: None,
    })
    .await
    .unwrap();
    let update = timeout(Duration::from_secs(5), rx.recv::<SessionUpdate>())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        matches!(update, SessionUpdate::Error { reason } if reason.contains("flsh's transcript cannot be read yet"))
    );
    task.await.unwrap().unwrap();
    database
        .execute("UPDATE session_meta SET title = 'Changed'", [])
        .unwrap();
    assert_eq!(
        source_summary(&agents).await.unwrap()[0].title.as_deref(),
        Some("Changed")
    );
}

fn page(earlier: u64, text: &[&str]) -> SessionSnapshot {
    SessionSnapshot {
        source: SessionSource::Owned(Box::default()),
        entries: text
            .iter()
            .map(|text| Entry::new(EntryKind::Agent, (*text).into()))
            .collect(),
        earlier,
        cursor: SessionCursor {
            start: earlier,
            next: earlier + text.len() as u64,
            fingerprint: fingerprint(
                earlier,
                &text
                    .iter()
                    .map(|text| Entry::new(EntryKind::Agent, (*text).into()))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        },
        resumed: false,
    }
}

fn request(after: Option<SessionCursor>) -> FollowSession {
    FollowSession {
        agent: "example".into(),
        session: "example".into(),
        last: 200,
        after,
    }
}

#[test]
fn fingerprints_cover_the_complete_compact_entry_encoding() {
    let mut entry = Entry::new(EntryKind::Thought, "thinking".into());
    entry.ms = Some(123);
    let entries = vec![entry];
    let encoded = rmp_serde::to_vec(&entries).unwrap();
    let mut expected = blake3::Hasher::new();
    expected.update(b"session-window/1");
    expected.update(&7_u64.to_le_bytes());
    expected.update(&(encoded.len() as u64).to_le_bytes());
    expected.update(&encoded);
    assert_eq!(
        fingerprint(7, &entries).unwrap(),
        expected.finalize().to_hex().to_string()
    );
    let before = page(7, &["thinking"]);
    let mut changed = before.clone();
    changed.entries[0].ms = Some(123);
    let (after, update) = next_update(changed, Some(&before), &request(None)).unwrap();
    assert_ne!(after.cursor.fingerprint, before.cursor.fingerprint);
    assert!(matches!(update, SessionUpdate::Delta(ref delta) if delta.entries[0].ms == Some(123)));
}

#[test]
fn structured_tool_updates_change_the_cursor_and_invalidate_old_resume_positions() {
    let mut before = page(0, &["command"]);
    before.entries[0].tool = Some(beep_proto::AgentToolCall {
        name: Some("command".into()),
        input: Some("echo example".into()),
        ..Default::default()
    });
    before.cursor.fingerprint = fingerprint(0, &before.entries).unwrap();
    let mut after = before.clone();
    after.entries[0].tool.as_mut().unwrap().result = Some("example".into());
    let (current, delta) = next_update(after.clone(), Some(&before), &request(None)).unwrap();
    assert_ne!(current.cursor.fingerprint, before.cursor.fingerprint);
    assert!(
        matches!(delta, SessionUpdate::Delta(ref delta) if delta.earlier == 0 && delta.entries[0].tool.as_ref().unwrap().result.as_deref() == Some("example"))
    );
    let (_, update) = next_update(after, None, &request(Some(before.cursor))).unwrap();
    assert!(matches!(update, SessionUpdate::Snapshot(ref snapshot) if !snapshot.resumed));
    let mut large = page(0, &["tool", "tool", "tool"]);

    for entry in &mut large.entries {
        entry.tool = Some(beep_proto::AgentToolCall {
            result: Some("x".repeat(RETAINED_BYTES / 2 - 128)),
            ..Default::default()
        });
    }

    let bounded = retained(large).unwrap();
    assert_eq!(bounded.earlier, 1);
    assert_eq!(bounded.entries.len(), 2);
}

#[test]
fn retention_counts_complete_encoded_entries_and_rejects_an_oversized_entry() {
    let entry = Entry::new(EntryKind::Tool, "x".repeat(RETAINED_BYTES / 2 - 64));
    let mut snapshot = page(0, &[]);
    snapshot.entries = vec![entry; 3];
    snapshot.cursor.next = 3;
    let retained = retained(snapshot).unwrap();
    assert_eq!(retained.earlier, 1);
    assert_eq!(retained.entries.len(), 2);
    assert!(rmp_serde::to_vec(&retained.entries).unwrap().len() <= RETAINED_BYTES);
    let mut snapshot = page(0, &[]);
    snapshot.entries = vec![Entry::new(EntryKind::Tool, "x".repeat(RETAINED_BYTES))];
    snapshot.cursor.next = 1;
    assert!(next_update(snapshot, None, &request(None)).is_err());
}

#[test]
fn deltas_replace_same_count_edits_and_truncate_the_suffix() {
    let before = page(3, &["one", "thi"]);
    let (retained, update) =
        next_update(page(3, &["one", "thinking"]), Some(&before), &request(None)).unwrap();
    let SessionUpdate::Delta(update) = update else {
        panic!("expected delta")
    };
    assert_eq!(update.earlier, 4);
    assert_eq!(update.entries[0].text, "thinking");
    assert_eq!(update.cursor, retained.cursor);
    let (_, update) = next_update(page(3, &["one"]), Some(&retained), &request(None)).unwrap();
    let SessionUpdate::Delta(update) = update else {
        panic!("expected delta")
    };
    assert_eq!(update.earlier, 4);
    assert!(update.entries.is_empty());
    assert_eq!(update.cursor.next, 4);
}

#[test]
fn moving_retention_keeps_overlap_and_gaps_reset_the_snapshot() {
    let before = page(3, &["one", "two", "three"]);
    let (_, update) = next_update(
        page(4, &["two", "three", "four"]),
        Some(&before),
        &request(None),
    )
    .unwrap();
    let SessionUpdate::Delta(update) = update else {
        panic!("expected delta")
    };
    assert_eq!(update.earlier, 6);
    assert_eq!(update.cursor.start, 4);
    assert_eq!(update.entries[0].text, "four");
    for fresh in [
        page(8, &["later"]),
        page(0, &["reset"]),
        page(0, &["reset", "before", "window"]),
    ] {
        assert!(matches!(
            next_update(fresh, Some(&before), &request(None)).unwrap().1,
            SessionUpdate::Snapshot(_)
        ));
    }
}

#[test]
fn reconnect_validates_the_retained_prefix_and_only_sends_new_entries() {
    let before = page(3, &["one", "two"]);
    let after = page(3, &["one", "two", "three"]);
    let (_, update) =
        next_update(after.clone(), None, &request(Some(before.cursor.clone()))).unwrap();
    let SessionUpdate::Snapshot(update) = update else {
        panic!("expected snapshot")
    };
    assert!(update.resumed);
    assert_eq!(update.earlier, 5);
    assert_eq!(update.entries[0].text, "three");
    for cursor in [
        page(3, &["changed", "two"]).cursor,
        page(0, &["missing"]).cursor,
        SessionCursor {
            next: 99,
            ..before.cursor.clone()
        },
    ] {
        let (_, update) = next_update(after.clone(), None, &request(Some(cursor))).unwrap();
        let SessionUpdate::Snapshot(update) = update else {
            panic!("expected snapshot")
        };
        assert!(!update.resumed);
        assert_eq!(update.earlier, 3);
        assert_eq!(update.entries.len(), 3);
    }
}

#[tokio::test]
async fn a_resume_window_can_extend_beyond_the_initial_two_hundred_entries() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("projects/project");
    std::fs::create_dir_all(&folder).unwrap();
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let line = |n| {
        if n % 2 == 0 {
            format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"entry {n}\"}}}}\n"
            )
        } else {
            format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"entry {n}\"}}]}}}}\n"
            )
        }
    };
    let mut text: String = (0..250).map(line).collect();
    let file = folder.join(format!("{id}.jsonl"));
    std::fs::write(&file, &text).unwrap();
    let agents = Agents {
        external: super::super::claude::External::locate_in(root.path()),
        ..Agents::default()
    };
    let mut request = FollowSession {
        agent: "claude".into(),
        session: id.into(),
        last: 200,
        after: None,
    };
    let raw = snapshot(&agents, &request).await.unwrap();
    assert_eq!(raw.entries.len(), 250);
    let (initial, _) = next_update(raw, None, &request).unwrap();
    assert_eq!(initial.entries.len(), 200);
    assert_eq!(initial.cursor.start, 50);
    text.push_str(&line(250));
    std::fs::write(file, text).unwrap();
    request.after = Some(initial.cursor);
    let raw = snapshot(&agents, &request).await.unwrap();
    let (retained, update) = next_update(raw, None, &request).unwrap();
    assert_eq!(retained.entries.len(), 201);
    let SessionUpdate::Snapshot(update) = update else {
        panic!("expected snapshot")
    };
    assert!(update.resumed);
    assert_eq!(update.earlier, 250);
    assert_eq!(update.entries.len(), 1);
}

#[tokio::test]
async fn an_external_transcript_pushes_deltas_and_reconnects_from_a_checkpoint() {
    use super::super::claude::External;
    use std::{fs::OpenOptions, io::Write};
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("projects/project");
    std::fs::create_dir_all(&folder).unwrap();
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let file = folder.join(format!("{id}.jsonl"));
    std::fs::write(&file, "{\"type\":\"user\",\"cwd\":\"/srv/project\",\"timestamp\":\"2026-10-29T08:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n").unwrap();
    let agents = Arc::new(Agents {
        external: External::locate_in(root.path()),
        ..Agents::default()
    });
    let mut request = FollowSession {
        agent: "claude".into(),
        session: id.into(),
        last: 200,
        after: None,
    };
    async fn connect(
        agents: Arc<Agents>,
        request: &FollowSession,
    ) -> (LinkTx, LinkRx, tokio::task::JoinHandle<Result<()>>) {
        let link = beep_proto::node::local_link();
        let task = tokio::spawn(serve_follow(link.node.0, link.node.1, agents));
        let (mut tx, rx) = link.caller;
        tx.send(request).await.unwrap();
        (tx, rx, task)
    }
    async fn next(rx: &mut LinkRx) -> SessionSnapshot {
        match timeout(Duration::from_secs(5), rx.recv::<SessionUpdate>())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        {
            SessionUpdate::Snapshot(snapshot) | SessionUpdate::Delta(snapshot) => *snapshot,
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }
    let (tx, mut rx, task) = connect(agents.clone(), &request).await;
    assert_eq!(next(&mut rx).await.entries[0].text, "hello");
    let mut writer = OpenOptions::new().append(true).open(&file).unwrap();
    writeln!(writer, "{{\"type\":\"assistant\",\"timestamp\":\"2026-10-29T08:00:01Z\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"world\"}}]}}}}").unwrap();
    let update = next(&mut rx).await;
    assert_eq!(update.earlier, 1);
    assert_eq!(update.entries[0].text, "world");
    request.after = Some(update.cursor);
    drop(tx);
    timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    writeln!(writer, "{{\"type\":\"user\",\"timestamp\":\"2026-10-29T08:00:02Z\",\"message\":{{\"role\":\"user\",\"content\":\"offline\"}}}}").unwrap();
    let (tx, mut rx, task) = connect(agents, &request).await;
    let snapshot = next(&mut rx).await;
    assert!(snapshot.resumed);
    assert_eq!(snapshot.earlier, 2);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].text, "offline");
    let original = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, original.replace("world", "changed")).unwrap();
    let edited = next(&mut rx).await;
    assert_eq!(edited.earlier, 1);
    assert_eq!(edited.entries[0].text, "changed");
    assert_eq!(edited.cursor.next, 3);
    std::fs::write(&file, original.lines().next().unwrap().to_owned() + "\n").unwrap();
    let truncated = next(&mut rx).await;
    assert_eq!(truncated.earlier, 1);
    assert!(truncated.entries.is_empty());
    assert_eq!(truncated.cursor.next, 1);
    drop(tx);
    timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
