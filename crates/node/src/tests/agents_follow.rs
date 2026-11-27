use super::*;
use beep_proto::node::agent::{Entry, EntryKind};

#[tokio::test]
async fn event_sources_are_shared_and_stop_with_the_last_subscription() {
    let root = tempfile::tempdir().unwrap();
    let agents = Arc::new(Agents {
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::locate_in(root.path()), crate::logs::found::Stores::at(Some(root.path()), None, None)),
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
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::locate_in(root.path()), crate::logs::found::Stores::at(Some(root.path()), None, None)),
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
async fn summary_cache_rescans_only_changed_harnesses_and_tracks_file_changes() {
    let root = tempfile::tempdir().unwrap();
    let codex = root.path().join("codex");
    let sessions = codex.join("sessions/2026/11/01");
    let flsh = root.path().join("flsh");
    let project = flsh.join("project");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let codex_file = sessions.join("rollout-example.jsonl");
    std::fs::write(&codex_file, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-example\",\"cwd\":\"/srv/project\",\"source\":\"cli\"}}\n").unwrap();
    let flsh_file = project.join("example.jsonl");
    let flsh_session = |id: &str, title: &str| {
        format!(
            "{{\"subtype\":\"session_metadata\",\"sessionId\":\"{id}\",\"metadata\":{{\"working_dir\":\"/srv/project\",\"title\":\"{title}\"}}}}\n{{\"type\":\"user\",\"timestamp\":\"2026-11-01T00:00:00Z\",\"message\":{{\"content\":\"Hello\"}}}}\n"
        )
    };
    std::fs::write(&flsh_file, flsh_session("flsh-example", "Original")).unwrap();
    let agents = Agents {
        occupancy: crate::occupancy::Occupancy::over(Default::default(), crate::logs::found::Stores::at(Some(&codex), None, Some(&flsh))),
        ..Agents::default()
    };
    let mut cache = SummaryCache::default();
    let initial = cache.read(&agents, &BTreeSet::new()).await.unwrap();
    assert_eq!(initial.len(), 2);

    // An unrelated harness write must not walk the flsh store again.
    std::fs::remove_file(&flsh_file).unwrap();
    std::fs::write(
        project.join("new.jsonl"),
        flsh_session("flsh-new", "New session"),
    )
    .unwrap();
    std::fs::write(
        codex.join("session_index.jsonl"),
        "{\"id\":\"codex-example\",\"thread_name\":\"Renamed session\"}\n",
    )
    .unwrap();
    let summary = cache
        .read(&agents, &BTreeSet::from([Agent::Codex]))
        .await
        .unwrap();
    assert_eq!(
        summary
            .iter()
            .find(|session| session.id == "codex-example")
            .unwrap()
            .title
            .as_deref(),
        Some("Renamed session")
    );
    assert!(summary.iter().any(|session| session.id == "flsh-example"));
    assert!(!summary.iter().any(|session| session.id == "flsh-new"));

    let summary = cache
        .read(&agents, &BTreeSet::from([Agent::Flsh]))
        .await
        .unwrap();
    assert!(!summary.iter().any(|session| session.id == "flsh-example"));
    assert!(summary.iter().any(|session| session.id == "flsh-new"));

    let renamed = sessions.join("rollout-renamed.jsonl");
    std::fs::rename(&codex_file, &renamed).unwrap();
    assert_eq!(
        cache
            .read(&agents, &BTreeSet::from([Agent::Codex]))
            .await
            .unwrap(),
        summary
    );
    std::fs::remove_file(&renamed).unwrap();
    let summary = cache
        .read(&agents, &BTreeSet::from([Agent::Codex]))
        .await
        .unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].id, "flsh-new");
}

#[tokio::test]
async fn cached_source_notifications_follow_file_creation_title_changes_and_removal() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions/2026/11/01");
    std::fs::create_dir_all(&sessions).unwrap();
    let agents = Arc::new(Agents {
        occupancy: crate::occupancy::Occupancy::over(Default::default(), crate::logs::found::Stores::at(Some(root.path()), None, None)),
        ..Agents::default()
    });
    let mut changes = agents.events.subscribe();
    let _source = agents.watch_event_sources().await.unwrap();
    async fn changed(changes: &mut tokio::sync::broadcast::Receiver<crate::events::Change>) {
        timeout(Duration::from_secs(5), async {
            loop {
                if changes.recv().await.unwrap().topic.name() == "agents" {
                    return;
                }
            }
        })
        .await
        .unwrap();
    }
    changed(&mut changes).await;

    let file = sessions.join("rollout-example.jsonl");
    std::fs::write(&file, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"example\",\"cwd\":\"/srv/project\",\"source\":\"cli\"}}\n").unwrap();
    changed(&mut changes).await;
    std::fs::write(
        root.path().join("session_index.jsonl"),
        "{\"id\":\"example\",\"thread_name\":\"Renamed session\"}\n",
    )
    .unwrap();
    changed(&mut changes).await;
    std::fs::remove_file(&file).unwrap();
    changed(&mut changes).await;
}

#[tokio::test]
async fn listings_follow_what_the_watcher_read_of_the_stores() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions/2026/11/01");
    std::fs::create_dir_all(&sessions).unwrap();
    let write = |id: &str| {
        std::fs::write(
            sessions.join(format!("rollout-{id}.jsonl")),
            format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"/srv/project\",\"source\":\"cli\"}}}}\n"),
        )
        .unwrap();
    };
    let agents = Arc::new(Agents {
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::locate_in(root.path()), crate::logs::found::Stores::at(Some(root.path()), None, None)),
        ..Agents::default()
    });
    write("first");
    assert_eq!(agents.sightings().await.unwrap().len(), 1);
    // Unwatched, no copy of a read is kept: the next listing sees the change.
    write("second");
    assert_eq!(agents.sightings().await.unwrap().len(), 2);

    let mut changes = agents.events.subscribe();
    let _source = agents.watch_event_sources().await.unwrap();
    async fn summarized(changes: &mut tokio::sync::broadcast::Receiver<crate::events::Change>) {
        timeout(Duration::from_secs(5), async {
            loop {
                if changes.recv().await.unwrap().topic.name() == "agents" {
                    return;
                }
            }
        })
        .await
        .unwrap();
    }
    summarized(&mut changes).await;
    assert_eq!(agents.sightings().await.unwrap().len(), 2);

    // Watched, the summary task's read answers: it is ahead of the ping.
    write("third");
    summarized(&mut changes).await;
    assert_eq!(agents.sightings().await.unwrap().len(), 3);
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
        occupancy: crate::occupancy::Occupancy::over(Default::default(), crate::logs::found::Stores::at(Some(root.path()), None, None)),
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
        occupancy: crate::occupancy::Occupancy::over(Default::default(), crate::logs::found::Stores::at(None, None, Some(root.path()))),
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
        patches: false,
        shown: true,
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
    let entries: Vec<_> = text
        .iter()
        .map(|text| Entry::new(EntryKind::Agent, (*text).into()))
        .collect();
    let (prefix_next, prefix_fingerprint) = if entries.is_empty() {
        (earlier, String::new())
    } else {
        (
            earlier + entries.len() as u64 - 1,
            reference::fingerprint(earlier, &entries[..entries.len() - 1]).unwrap(),
        )
    };
    SessionSnapshot {
        source: SessionSource::Owned(Box::default()),
        cursor: SessionCursor {
            start: earlier,
            next: earlier + entries.len() as u64,
            fingerprint: reference::fingerprint(earlier, &entries).unwrap(),
            prefix_next,
            prefix_fingerprint,
        },
        entries,
        earlier,
        resumed: false,
        controls: false,
    }
}

/// The window a reader holds after receiving `snapshot`.
fn held(snapshot: &SessionSnapshot) -> Window {
    let (window, _) = retain(snapshot.clone(), None).unwrap();
    Window {
        cursor: snapshot.cursor.clone(),
        ..window
    }
}

fn request(after: Option<SessionCursor>) -> FollowSession {
    FollowSession {
        agent: "example".into(),
        session: "example".into(),
        last: 200,
        after,
        patches: false,
        shown: true,
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
        fingerprint(7, 1, &rmp_serde::to_vec(&entries[0]).unwrap()).unwrap(),
        expected.finalize().to_hex().to_string()
    );
    let before = page(7, &["thinking"]);
    let mut changed = before.clone();
    changed.entries[0].ms = Some(123);
    let (after, update) = next_update(changed, Some(held(&before)), &request(None)).unwrap();
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
    before.cursor.fingerprint = reference::fingerprint(0, &before.entries).unwrap();
    let mut after = before.clone();
    after.entries[0].tool.as_mut().unwrap().result = Some("example".into());
    let (current, delta) = next_update(after.clone(), Some(held(&before)), &request(None)).unwrap();
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

    let (bounded, _) = retain(large, None).unwrap();
    assert_eq!(bounded.earlier, 1);
    assert_eq!(bounded.entries.len(), 2);
}

#[test]
fn retention_counts_complete_encoded_entries_and_rejects_an_oversized_entry() {
    let entry = Entry::new(EntryKind::Tool, "x".repeat(RETAINED_BYTES / 2 - 64));
    let mut snapshot = page(0, &[]);
    snapshot.entries = vec![entry; 3];
    snapshot.cursor.next = 3;
    let (retained, _) = retain(snapshot, None).unwrap();
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
    let (retained, update) = next_update(
        page(3, &["one", "thinking"]),
        Some(held(&before)),
        &request(None),
    )
    .unwrap();
    let SessionUpdate::Delta(update) = update else {
        panic!("expected delta")
    };
    assert_eq!(update.earlier, 4);
    assert_eq!(update.entries[0].text, "thinking");
    assert_eq!(update.cursor, retained.cursor);
    let (_, update) = next_update(page(3, &["one"]), Some(retained), &request(None)).unwrap();
    let SessionUpdate::Delta(update) = update else {
        panic!("expected delta")
    };
    assert_eq!(update.earlier, 4);
    assert!(update.entries.is_empty());
    assert_eq!(update.cursor.next, 4);
}

#[test]
fn readers_over_the_cache_budget_evict_the_least_recently_updated_window() {
    let mut windows = Windows::default();
    let window = || held(&page(0, &["one", "two"]));
    let cost = window().cost();
    windows.keep(1, window(), 2 * cost);
    windows.keep(2, window(), 2 * cost);
    windows.keep(3, window(), 2 * cost);
    assert_eq!(windows.bytes, 2 * cost);

    let oldest = windows.take(1).unwrap();
    assert!(
        oldest.entries.is_empty(),
        "the oldest window dropped its cache"
    );
    assert_eq!(
        oldest.cursor.next, 2,
        "it still knows what its reader holds"
    );
    assert!(!windows.take(3).unwrap().entries.is_empty());
    assert_eq!(windows.bytes, cost, "only the second window still caches");
}

#[test]
fn an_evicted_window_sends_only_new_entries_and_resends_after_an_edit() {
    let before = page(3, &["one", "two"]);
    let evicted = || {
        let mut window = held(&before);
        window.evict();
        window
    };

    let (retained, update) = next_update(
        page(3, &["one", "two", "three"]),
        Some(evicted()),
        &request(None),
    )
    .unwrap();
    let SessionUpdate::Delta(update) = update else {
        panic!("expected delta")
    };
    assert_eq!(update.earlier, 5);
    assert_eq!(update.entries.len(), 1);
    assert!(retained.caches_held(), "it caches again");

    let (_, update) =
        next_update(page(3, &["one", "edited"]), Some(evicted()), &request(None)).unwrap();
    let SessionUpdate::Delta(update) = update else {
        panic!("expected delta")
    };
    assert_eq!(update.earlier, 4);
    assert_eq!(update.entries.len(), 1);
    assert_eq!(update.entries[0].text, "edited");
}

#[test]
fn moving_retention_keeps_overlap_and_gaps_reset_the_snapshot() {
    let before = page(3, &["one", "two", "three"]);
    let (_, update) = next_update(
        page(4, &["two", "three", "four"]),
        Some(held(&before)),
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
            next_update(fresh, Some(held(&before)), &request(None))
                .unwrap()
                .1,
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
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::locate_in(root.path()), Default::default()),
        ..Agents::default()
    };
    let mut request = FollowSession {
        agent: "claude".into(),
        session: id.into(),
        last: 200,
        after: None,
        patches: false,
        shown: true,
    };
    let raw = snapshot(&agents, &request, read_retained()).await.unwrap();
    assert_eq!(raw.entries.len(), 250);
    let (initial, _) = next_update(raw, None, &request).unwrap();
    assert_eq!(initial.cursor.next - initial.cursor.start, 200);
    assert_eq!(initial.cursor.start, 50);
    text.push_str(&line(250));
    std::fs::write(file, text).unwrap();
    request.after = Some(initial.cursor);
    let raw = snapshot(&agents, &request, read_retained()).await.unwrap();
    let (retained, update) = next_update(raw, None, &request).unwrap();
    assert_eq!(retained.cursor.next - retained.cursor.start, 201);
    let SessionUpdate::Snapshot(update) = update else {
        panic!("expected snapshot")
    };
    assert!(update.resumed);
    assert_eq!(update.earlier, 250);
    assert_eq!(update.entries.len(), 1);
}

#[test]
fn patches_send_append_text_without_source_and_isolate_single_entry_edits() {
    let mut req = request(None);
    req.patches = true;

    let (w1, first) = next_update(page(0, &["one", "two", "hello"]), None, &req).unwrap();
    let SessionUpdate::Snapshot(snap) = first else {
        panic!("expected initial snapshot");
    };
    assert!(snap.controls);

    // Appending text to the tail entry emits a tiny AppendText patch with source: None.
    let (w2, second) =
        next_update(page(0, &["one", "two", "hello world"]), Some(w1), &req).unwrap();
    let SessionUpdate::Patch(patch) = second else {
        panic!("expected patch, got {second:?}");
    };
    assert!(patch.source.is_none());
    assert_eq!(
        patch.ops,
        vec![EntryPatch::AppendText {
            index: 2,
            text: " world".into(),
        }]
    );

    // Editing an earlier entry leaves trailing entries out of the patch.
    let mut edited = page(0, &["one", "two", "hello world"]);
    edited.entries[0].tool = Some(beep_proto::AgentToolCall {
        result: Some("done".into()),
        ..Default::default()
    });
    let (w3, third) = next_update(edited, Some(w2), &req).unwrap();
    let SessionUpdate::Patch(patch) = third else {
        panic!("expected patch");
    };
    assert!(patch.source.is_none());
    assert_eq!(patch.ops.len(), 1);
    assert!(matches!(patch.ops[0], EntryPatch::Replace { index: 0, .. }));

    // Truncating emits Truncate without resending retained entries.
    let (_, fourth) = next_update(page(0, &["one"]), Some(w3), &req).unwrap();
    let SessionUpdate::Patch(patch) = fourth else {
        panic!("expected patch");
    };
    assert!(
        patch
            .ops
            .iter()
            .any(|op| matches!(op, EntryPatch::Truncate { next: 1 }))
    );
}

#[test]
fn reconnect_resumes_from_prefix_when_only_tail_entry_mutated() {
    let mut req = request(None);
    req.patches = true;

    let (initial, _) = next_update(page(3, &["one", "two", "hel"]), None, &req).unwrap();
    let after = page(3, &["one", "two", "hello", "four"]);

    // Patch-capable client validates prefix [3, 5) and receives only entries [5, 7).
    req.after = Some(initial.cursor.clone());
    let (_, resumed) = next_update(after.clone(), None, &req).unwrap();
    let SessionUpdate::Snapshot(snap) = resumed else {
        panic!("expected snapshot");
    };
    assert!(snap.resumed);
    assert_eq!(snap.earlier, 5);
    assert_eq!(snap.entries.len(), 2);
    assert_eq!(snap.entries[0].text, "hello");
    assert_eq!(snap.entries[1].text, "four");

    // Legacy client without patches falls back to a full snapshot.
    let legacy = request(Some(initial.cursor));
    let (_, full) = next_update(after, None, &legacy).unwrap();
    let SessionUpdate::Snapshot(snap) = full else {
        panic!("expected snapshot");
    };
    assert!(!snap.resumed);
    assert_eq!(snap.earlier, 3);
    assert_eq!(snap.entries.len(), 4);
}

/// The full re-encoding builder this module replaced, kept to prove identical output.
mod reference {
    use super::*;

    pub(super) fn fingerprint(start: u64, entries: &[Entry]) -> Result<String> {
        let wire = rmp_serde::to_vec(entries)?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"session-window/1");
        hash.update(&start.to_le_bytes());
        hash.update(&u64::try_from(wire.len())?.to_le_bytes());
        hash.update(&wire);
        Ok(hash.finalize().to_hex().to_string())
    }

    fn window(mut snapshot: SessionSnapshot, start: u64) -> Result<SessionSnapshot> {
        let skip = start
            .saturating_sub(snapshot.earlier)
            .min(snapshot.entries.len() as u64) as usize;
        snapshot.entries.drain(..skip);
        snapshot.earlier += skip as u64;
        snapshot.cursor.start = snapshot.earlier;
        snapshot.cursor.fingerprint = fingerprint(snapshot.earlier, &snapshot.entries)?;
        if snapshot.entries.is_empty() {
            snapshot.cursor.prefix_next = snapshot.earlier;
            snapshot.cursor.prefix_fingerprint.clear();
        } else {
            let prefix_len = snapshot.entries.len() - 1;
            snapshot.cursor.prefix_next = snapshot.earlier + prefix_len as u64;
            snapshot.cursor.prefix_fingerprint =
                fingerprint(snapshot.earlier, &snapshot.entries[..prefix_len])?;
        }
        Ok(snapshot)
    }

    fn validates(snapshot: &SessionSnapshot, cursor: &SessionCursor) -> Result<bool> {
        if cursor.start < snapshot.earlier
            || cursor.next < cursor.start
            || cursor.next > snapshot.cursor.next
        {
            return Ok(false);
        }
        let start = (cursor.start - snapshot.earlier) as usize;
        let end = (cursor.next - snapshot.earlier) as usize;
        Ok(fingerprint(cursor.start, &snapshot.entries[start..end])? == cursor.fingerprint)
    }

    fn since(snapshot: &SessionSnapshot, previous: &SessionSnapshot) -> SessionSnapshot {
        let offset = (snapshot.earlier - previous.earlier) as usize;
        let same = if snapshot.cursor == previous.cursor {
            snapshot.entries.len()
        } else {
            snapshot
                .entries
                .iter()
                .zip(previous.entries.iter().skip(offset))
                .take_while(|(new, old)| new == old)
                .count()
        };
        suffix(snapshot, same, false)
    }

    fn suffix(snapshot: &SessionSnapshot, skip: usize, resumed: bool) -> SessionSnapshot {
        SessionSnapshot {
            source: snapshot.source.clone(),
            entries: snapshot.entries[skip..].to_vec(),
            earlier: snapshot.earlier + skip as u64,
            cursor: snapshot.cursor.clone(),
            resumed,
            controls: false,
        }
    }

    fn retained(mut snapshot: SessionSnapshot) -> Result<SessionSnapshot> {
        let mut bytes = 5usize;
        let mut kept = 0;
        for entry in snapshot.entries.iter().rev() {
            let next = bytes + rmp_serde::to_vec(entry)?.len();
            if next > RETAINED_BYTES {
                break;
            }
            bytes = next;
            kept += 1;
        }
        if kept == 0 && !snapshot.entries.is_empty() {
            bail!("a transcript entry exceeds the retained window limit");
        }
        let removed = snapshot.entries.len() - kept;
        snapshot.entries.drain(..removed);
        snapshot.earlier += removed as u64;
        snapshot.cursor.start = snapshot.earlier;
        Ok(snapshot)
    }

    pub(super) fn next_update(
        snapshot: SessionSnapshot,
        previous: Option<&SessionSnapshot>,
        request: &FollowSession,
    ) -> Result<(SessionSnapshot, SessionUpdate)> {
        let snapshot = retained(snapshot)?;
        let initial_start = initial_start(&snapshot.entries, snapshot.earlier, request.last);
        if let Some(previous) = previous {
            if snapshot.earlier <= previous.cursor.next && previous.earlier < snapshot.cursor.next {
                let snapshot = window(snapshot, previous.earlier)?;
                let delta = since(&snapshot, previous);
                return Ok((snapshot, SessionUpdate::Delta(Box::new(delta))));
            }
        } else if let Some(after) = &request.after
            && validates(&snapshot, after)?
        {
            let snapshot = window(snapshot, after.start)?;
            let skip = (after.next - snapshot.earlier) as usize;
            let update = suffix(&snapshot, skip, true);
            return Ok((snapshot, SessionUpdate::Snapshot(Box::new(update))));
        }
        let snapshot = window(snapshot, initial_start)?;
        Ok((
            snapshot.clone(),
            SessionUpdate::Snapshot(Box::new(snapshot)),
        ))
    }
}

#[test]
fn incremental_updates_send_the_same_bytes_as_full_reencoding() {
    let read = |earlier: u64, entries: &[Entry]| SessionSnapshot {
        source: SessionSource::Owned(Box::default()),
        entries: entries.to_vec(),
        earlier,
        cursor: SessionCursor {
            start: earlier,
            next: earlier + entries.len() as u64,
            fingerprint: String::new(),
            prefix_next: earlier,
            prefix_fingerprint: String::new(),
        },
        resumed: false,
        controls: false,
    };
    let text = |n: usize| Entry::new(EntryKind::Agent, format!("entry {n}"));
    let mut entries: Vec<_> = (0..300).map(text).collect();
    let mut steps = vec![(0, entries.clone())];
    entries.push(text(300));
    steps.push((0, entries.clone()));
    entries.last_mut().unwrap().text.push_str(" streamed");
    steps.push((0, entries.clone()));
    entries[290].tool = Some(beep_proto::AgentToolCall {
        result: Some("finished".into()),
        ..Default::default()
    });
    steps.push((0, entries.clone()));
    steps.push((0, entries.clone()));
    entries.truncate(295);
    steps.push((0, entries.clone()));
    steps.push((40, entries[40..].to_vec()));
    // Large entries push the byte limit past the reader's window start.
    entries.extend((0..3).map(|n| Entry::new(EntryKind::Tool, "x".repeat(RETAINED_BYTES / 3 - n))));
    steps.push((40, entries[40..].to_vec()));
    entries.push(text(1));
    steps.push((40, entries[40..].to_vec()));
    steps.push((0, vec![text(7), text(8)]));
    steps.push((0, vec![text(7), text(9), text(10)]));

    let mut old: Option<SessionSnapshot> = None;
    let mut new: Option<Window> = None;
    let mut cursors = Vec::new();

    for (earlier, entries) in &steps {
        let (old_window, old_update) =
            reference::next_update(read(*earlier, entries), old.as_ref(), &request(None)).unwrap();
        let (new_window, new_update) =
            next_update(read(*earlier, entries), new.take(), &request(None)).unwrap();
        assert_eq!(
            rmp_serde::to_vec(&old_update).unwrap(),
            rmp_serde::to_vec(&new_update).unwrap()
        );
        assert_eq!(old_window.cursor, new_window.cursor);
        let whole = rmp_serde::to_vec(&new_window.entries).unwrap();
        let header = whole.len() - new_window.wire.len();
        assert_eq!(whole[header..], new_window.wire);
        cursors.push(new_window.cursor.clone());
        old = Some(old_window);
        new = Some(new_window);
    }

    // Reconnects from every checkpoint resume or restart exactly as before.
    for (earlier, entries) in &steps {
        for cursor in &cursors {
            let request = request(Some(cursor.clone()));
            let (old_window, old_update) =
                reference::next_update(read(*earlier, entries), None, &request).unwrap();
            let (new_window, new_update) =
                next_update(read(*earlier, entries), None, &request).unwrap();
            assert_eq!(
                rmp_serde::to_vec(&old_update).unwrap(),
                rmp_serde::to_vec(&new_update).unwrap()
            );
            assert_eq!(old_window.cursor, new_window.cursor);
        }
    }
}

/// Times one streaming update against the full re-encoding builder, counting the
/// replaced window's drop in both:
/// `cargo test -p beep-node --release --lib streaming_update_cost -- --ignored --nocapture`.
#[test]
#[ignore = "timing"]
fn streaming_update_cost() {
    let entries: Vec<_> = (0..RETAINED_ENTRIES)
        .map(|n| {
            let mut entry = Entry::new(EntryKind::Tool, format!("step {n} ").repeat(40));
            entry.tool = Some(beep_proto::AgentToolCall {
                name: Some("command".into()),
                input: Some(format!("cargo test {n}")),
                result: Some("ok ".repeat(150)),
                ..Default::default()
            });
            entry
        })
        .collect();
    let read = |entries: &[Entry]| SessionSnapshot {
        source: SessionSource::Owned(Box::default()),
        entries: entries.to_vec(),
        earlier: 0,
        cursor: SessionCursor {
            start: 0,
            next: entries.len() as u64,
            fingerprint: String::new(),
            prefix_next: 0,
            prefix_fingerprint: String::new(),
        },
        resumed: false,
        controls: false,
    };
    let (whole, _) = retain(read(&entries), None).unwrap();
    let whole = SessionCursor {
        fingerprint: fingerprint(whole.earlier, whole.entries.len(), &whole.wire).unwrap(),
        ..whole.cursor
    };
    let rounds = 100;

    for after in [None, Some(whole)] {
        let request = request(after);
        let mut streamed = entries.clone();
        let (mut old, _) = reference::next_update(read(&streamed), None, &request).unwrap();
        let (mut new, _) = next_update(read(&streamed), None, &request).unwrap();
        let held = new.cursor.next - new.cursor.start;
        let (mut before, mut after) = (Vec::new(), Vec::new());

        for round in 0..rounds {
            streamed
                .last_mut()
                .unwrap()
                .text
                .push_str(&format!(" token {round}"));
            let snapshot = read(&streamed);
            let started = std::time::Instant::now();
            old = reference::next_update(snapshot, Some(&old), &request)
                .unwrap()
                .0;
            before.push(started.elapsed());
            let snapshot = read(&streamed);
            let started = std::time::Instant::now();
            new = next_update(snapshot, Some(new), &request).unwrap().0;
            after.push(started.elapsed());
        }

        before.sort();
        after.sort();
        println!(
            "{held} held entries, median per update: {:?} before, {:?} after",
            before[before.len() / 2],
            after[after.len() / 2]
        );
    }
}

/// A snapshot read of a session's retained entries, found by listing.
fn read_retained() -> Read<'static> {
    Read {
        followed: None,
        stale: true,
        retained: RETAINED_ENTRIES,
        from: None,
    }
}

#[tokio::test]
async fn callers_asking_during_a_store_read_share_it() {
    let root = tempfile::tempdir().unwrap();
    let agents = Agents {
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::locate_in(root.path()), crate::logs::found::Stores::at(Some(root.path()), None, None)),
        ..Agents::default()
    };
    let (first, second) = (agents.sightings(), crate::agents::reach::sighted_folders(&agents.occupancy));
    tokio::pin!(first, second);
    assert!(futures::poll!(first.as_mut()).is_pending());
    assert!(futures::poll!(second.as_mut()).is_pending());

    let running = agents.occupancy.reading_now().unwrap();
    // This handle and the two callers'.
    assert_eq!(Arc::strong_count(&running), 3);
    drop(running);

    first.await.unwrap();
    second.await.unwrap();
}

#[test]
fn initial_snapshot_counts_tool_and_thought_runs_as_single_rows() {
    let turn = |n: usize| {
        let mut items = vec![
            Entry::new(EntryKind::User, format!("prompt {n}")),
            Entry::new(EntryKind::Thought, format!("thought {n}")),
        ];
        items.extend((0..35).map(|step| Entry::new(EntryKind::Tool, format!("tool {n}:{step}"))));
        items.push(Entry::new(EntryKind::Agent, format!("reply {n}")));
        items
    };

    // Six tool-heavy turns (228 raw entries) are 18 folded rows, so `last: 65`
    // includes the whole session (`earlier == 0`) in one initial snapshot.
    let short: Vec<_> = (0..6).flat_map(turn).collect();
    assert_eq!(short.len(), 228);
    let mut short_snap = page(0, &[]);
    short_snap.entries = short;
    short_snap.cursor.next = 228;
    let mut req = request(None);
    req.last = 65;
    let (_, update) = next_update(short_snap, None, &req).unwrap();
    let SessionUpdate::Snapshot(snap) = update else {
        panic!("expected snapshot");
    };
    assert_eq!(snap.earlier, 0);
    assert_eq!(snap.entries.len(), 228);

    // Three folded rows from the tail (`prompt`, `thought + 35 tools`, `reply`)
    // keep the entire progress run intact without pulling in earlier turns.
    let two_turns: Vec<_> = (0..2).flat_map(turn).collect();
    let mut two_snap = page(0, &[]);
    two_snap.entries = two_turns;
    two_snap.cursor.next = 76;
    req.last = 3;
    let (_, update) = next_update(two_snap, None, &req).unwrap();
    let SessionUpdate::Snapshot(snap) = update else {
        panic!("expected snapshot");
    };
    assert_eq!(snap.earlier, 38);
    assert_eq!(snap.entries.len(), 38);
    assert_eq!(snap.entries[0].kind, EntryKind::User);
}
