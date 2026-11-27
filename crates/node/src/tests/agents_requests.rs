use super::*;
use crate::agents::Parts;
use beep_proto::node::agent::EntryKind;
use serde_json::json;
use std::{fs, path::Path};

const CODEX_ID: &str = "01a199e3-03f2-7820-a29e-067ae8c4ec85";
const PI_ID: &str = "pi-session-1";

fn lines(values: &[serde_json::Value]) -> String {
    values.iter().map(|line| format!("{line}\n")).collect()
}

/// A fabricated Codex rollout: a prompt, reasoning, a call and its
/// output, an answer, then a second prompt and answer.
fn codex_rollout() -> String {
    let user = |text: &str| json!({"type": "event_msg", "payload": {"type": "user_message", "message": text}});
    let said = |text: &str| {
        json!({"type": "response_item", "payload": {"type": "message",
        "role": "assistant", "content": [{"type": "output_text", "text": text}]}})
    };
    lines(&[
        json!({"type": "session_meta", "payload": {"id": CODEX_ID, "cwd": "/srv/project",
            "timestamp": "2026-11-02T01:32:34.000Z", "source": "cli"}}),
        json!({"type": "response_item", "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "<environment_context>"}]}}),
        user("fix the retry flag"),
        json!({"type": "response_item", "payload": {"type": "reasoning",
            "summary": [{"type": "summary_text", "text": "Looking at the flag"}]}}),
        json!({"type": "response_item", "payload": {"type": "function_call", "name": "exec_command",
            "arguments": "{\"cmd\":\"cargo test\"}", "call_id": "c1"}}),
        json!({"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1", "output": "ok"}}),
        said("The flag is fixed."),
        user("now the docs"),
        said("Docs updated."),
    ])
}

fn pi_session() -> String {
    lines(&[
        json!({"type": "session", "id": PI_ID, "cwd": "/srv/project", "timestamp": "2026-11-02T01:32:34.000Z"}),
        json!({"type": "message", "message": {"role": "user", "content": [{"type": "text", "text": "list the files"}]}}),
        json!({"type": "message", "message": {"role": "assistant", "content": [
            {"type": "thinking", "thinking": "I will run ls"},
            {"type": "toolCall", "id": "t1", "name": "bash", "arguments": {"command": "ls"}},
            {"type": "text", "text": "Here they are."}]}}),
        json!({"type": "message", "message": {"role": "toolResult", "toolCallId": "t1", "isError": true,
            "content": [{"type": "text", "text": "boom"}]}}),
    ])
}

fn agents(codex: &Path, pi: &Path) -> Agents {
    let nothing = tempfile::tempdir().expect("a temp folder");
    Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(
            crate::logs::claude::Store::at(nothing.path()),
            crate::logs::found::Stores::at(Some(codex), Some(pi), None),
        ),
        ..Parts::default()
    })
}

async fn read(agents: &Agents, id: &str, last: u32) -> SessionsReply {
    transcript_reply(&agents.transcripts, &SessionId::from(id), last, true, None).await
}

#[tokio::test]
async fn a_codex_session_started_outside_beep_lists_then_reads() {
    let codex = tempfile::tempdir().expect("a temp folder");
    let pi = tempfile::tempdir().expect("a temp folder");
    let dir = codex.path().join("sessions/2026/11/02");
    fs::create_dir_all(&dir).expect("folders");
    fs::write(
        dir.join(format!("rollout-2026-11-02T02-32-34-{CODEX_ID}.jsonl")),
        codex_rollout(),
    )
    .expect("a rollout");
    let agents = agents(codex.path(), pi.path());

    let listed = agents
        .listings
        .stores_external("codex", 10, Some(CODEX_ID.into()))
        .await
        .expect("the listing");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, CODEX_ID);
    assert_eq!(listed[0].agent, "codex");

    let SessionsReply::Transcript { entries, earlier } = read(&agents, CODEX_ID, 50).await else {
        panic!("a transcript");
    };
    let shown: Vec<(EntryKind, &str)> = entries
        .iter()
        .map(|entry| (entry.kind, entry.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (EntryKind::User, "fix the retry flag"),
            (EntryKind::Thought, "Looking at the flag"),
            (EntryKind::Tool, "exec_command: cargo test"),
            (EntryKind::Agent, "The flag is fixed."),
            (EntryKind::User, "now the docs"),
            (EntryKind::Agent, "Docs updated."),
        ]
    );
    assert_eq!(earlier, 0);

    let SessionsReply::Transcript { entries, earlier } = read(&agents, CODEX_ID, 2).await else {
        panic!("a transcript");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].text, "Docs updated.");
    assert_eq!(earlier, 4);
}

/// `Transcripts` built from the node's own `Sessions`, `Listings` and
/// `Family` reads a found session's file, and its reply wraps the read.
#[tokio::test]
async fn a_transcripts_owner_reads_a_found_session_without_agents() {
    let codex = tempfile::tempdir().expect("a temp folder");
    let pi = tempfile::tempdir().expect("a temp folder");
    let dir = codex.path().join("sessions/2026/11/02");
    fs::create_dir_all(&dir).expect("folders");
    fs::write(
        dir.join(format!("rollout-2026-11-02T02-32-34-{CODEX_ID}.jsonl")),
        codex_rollout(),
    )
    .expect("a rollout");
    let agents = agents(codex.path(), pi.path());
    let transcripts = crate::agents::transcripts::Transcripts::new(
        agents.sessions.clone(),
        agents.listings.clone(),
        agents.family.clone(),
    );
    let id = SessionId::from(CODEX_ID);

    let (entries, earlier) = transcripts
        .transcript_window(&id, 50, None, None, true)
        .await
        .expect("the read")
        .expect("the found session");
    assert_eq!(entries.len(), 6);
    assert_eq!(earlier, 0);

    let SessionsReply::Transcript {
        entries: wrapped, ..
    } = transcript_reply(&transcripts, &id, 50, true, None).await
    else {
        panic!("a transcript");
    };
    assert_eq!(wrapped.len(), 6);
}

#[tokio::test]
async fn pages_before_an_entry_cover_a_found_session_without_a_gap_or_a_repeat() {
    let codex = tempfile::tempdir().expect("a temp folder");
    let pi = tempfile::tempdir().expect("a temp folder");
    let dir = codex.path().join("sessions/2026/11/02");
    fs::create_dir_all(&dir).expect("folders");
    fs::write(
        dir.join(format!("rollout-2026-11-02T02-32-34-{CODEX_ID}.jsonl")),
        codex_rollout(),
    )
    .expect("a rollout");
    let agents = agents(codex.path(), pi.path());
    let SessionsReply::Transcript { entries: all, .. } = read(&agents, CODEX_ID, 50).await else {
        panic!("a transcript");
    };

    let mut seen = Vec::new();
    let mut before = None;

    loop {
        let SessionsReply::Transcript { entries, earlier } = transcript_reply(
            &agents.transcripts,
            &SessionId::from(CODEX_ID),
            4,
            true,
            before,
        )
        .await
        else {
            panic!("a transcript");
        };

        if let Some(end) = before {
            assert_eq!(
                u64::from(earlier) + entries.len() as u64,
                end,
                "ends at before"
            );
        }

        if entries.is_empty() {
            break;
        }

        seen.splice(0..0, entries);
        before = Some(u64::from(earlier));

        if earlier == 0 {
            break;
        }
    }

    assert_eq!(seen, all);
}

#[tokio::test]
async fn a_pi_session_reads_and_marks_a_failed_call() {
    let codex = tempfile::tempdir().expect("a temp folder");
    let pi = tempfile::tempdir().expect("a temp folder");
    let dir = pi.path().join("--srv-project--");
    fs::create_dir_all(&dir).expect("folders");
    fs::write(
        dir.join(format!("2026-11-02T01-32-34-000Z_{PI_ID}.jsonl")),
        pi_session(),
    )
    .expect("a session");
    let agents = agents(codex.path(), pi.path());
    let SessionsReply::Transcript { entries, .. } = read(&agents, PI_ID, 50).await else {
        panic!("a transcript");
    };
    let shown: Vec<(EntryKind, &str)> = entries
        .iter()
        .map(|entry| (entry.kind, entry.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (EntryKind::User, "list the files"),
            (EntryKind::Thought, "I will run ls"),
            (EntryKind::Tool, "bash: ls (failed)"),
            (EntryKind::Agent, "Here they are."),
        ]
    );
}

#[tokio::test]
async fn an_unknown_session_is_no_session() {
    let codex = tempfile::tempdir().expect("a temp folder");
    let pi = tempfile::tempdir().expect("a temp folder");
    let agents = agents(codex.path(), pi.path());
    let SessionsReply::Error { message } = read(&agents, "nothing", 5).await else {
        panic!("an error");
    };
    assert_eq!(message, "there is no session nothing");
}

#[tokio::test]
async fn a_native_codex_conversation_reads_only_its_own_messages_and_outputs() {
    let codex = tempfile::tempdir().expect("a temp folder");
    let pi = tempfile::tempdir().expect("a temp folder");
    let dir = codex.path().join("sessions/2026/11/02");
    fs::create_dir_all(&dir).expect("folders");
    let text = lines(&[
        json!({"type":"session_meta","payload":{"id":CODEX_ID,"cwd":"/srv/project","source":{"subagent":{}},"agent_path":"/root/check","subagent_history_start_ordinal":3}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"Parent history"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"Parent answer"}]}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"Check the fixture"}}),
        json!({"type":"event_msg","payload":{"type":"task_started"}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cargo test\"}","call_id":"c1"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"1 test passed"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"The fixture passes."}]}}),
        json!({"type":"event_msg","payload":{"type":"task_complete"}}),
    ]);
    fs::write(dir.join(format!("rollout-example-{CODEX_ID}.jsonl")), text).expect("rollout");
    let agents = agents(codex.path(), pi.path());
    let id = format!("subagent:codex:{CODEX_ID}");
    let listed = agents
        .listings
        .stores_external("codex", 10, None)
        .await
        .expect("roots");
    assert!(listed.is_empty());
    let listed = agents
        .listings
        .stores_external("codex", 1, Some(SessionId::from(id.clone())))
        .await
        .expect("native session");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].presence,
        beep_proto::node::agent::Presence::Stopped
    );
    let SessionsReply::Transcript { entries, earlier } = read(&agents, &id, 50).await else {
        panic!("transcript")
    };
    assert_eq!(earlier, 0);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].text, "Check the fixture");
    assert_eq!(
        entries[1].text,
        "exec_command: cargo test [completed]\n1 test passed"
    );
    assert_eq!(entries[2].text, "The fixture passes.");
    let SessionsReply::Transcript {
        entries: tail,
        earlier,
    } = read(&agents, &id, 1).await
    else {
        panic!("last entry")
    };
    assert_eq!(earlier, 2);
    assert_eq!(tail, entries[2..]);
    let SessionsReply::Transcript {
        entries: first,
        earlier,
    } = transcript_reply(&agents.transcripts, &SessionId::from(&id), 2, true, Some(2)).await
    else {
        panic!("older entries")
    };
    assert_eq!(earlier, 0);
    assert_eq!(first, entries[..2]);
}

#[tokio::test]
async fn a_folder_listing_shows_an_edit_made_after_the_status_was_kept() {
    use crate::agents::checkout::tests::{has_git, repo};
    use std::time::Duration;

    if !has_git() {
        return;
    }

    let (_dir, work) = repo("folder");
    let agents = Agents::new(Parts {
        folders: vec![work.clone()],
        ..Parts::default()
    });
    let root = work.to_str().unwrap();
    let readme = |reply: SessionsReply| match reply {
        SessionsReply::Folder { folder } => {
            folder
                .entries
                .iter()
                .find(|e| e.name == "README.md")
                .unwrap()
                .change
        }
        other => panic!("not a folder: {other:?}"),
    };

    // Listed until the checkout's watch holds and keeps the status.
    for _ in 0..3 {
        let reply = folder_reply(&agents, root, String::new(), false, 0).await;
        assert_eq!(readme(reply), None);
    }

    fs::write(work.join("README.md"), "hi\nthere\n").unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let reply = folder_reply(&agents, root, String::new(), false, 0).await;

            if readme(reply).is_some() {
                return;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the edit shows");
}

#[test]
fn a_refused_request_and_one_whose_caller_left_end_differently() {
    let mut trace = beep_proto::logging::RequestTrace::new("test");
    let refused = SessionsReply::Error {
        message: "no such session".into(),
    };
    let fine = SessionsReply::NoFile {};
    let left = Err(anyhow::anyhow!("the caller left"));

    assert_eq!(end_trace(&mut trace, &Ok(()), &refused), "refused");
    assert_eq!(end_trace(&mut trace, &left, &refused), "error");
    assert_eq!(end_trace(&mut trace, &left, &fine), "error");
    assert_eq!(end_trace(&mut trace, &Ok(()), &fine), "ok");
}

#[tokio::test]
async fn a_found_session_reads_from_the_listings_alone() {
    let codex = tempfile::tempdir().expect("a temp folder");
    let nothing = tempfile::tempdir().expect("a temp folder");
    let dir = codex.path().join("sessions/2026/11/02");
    fs::create_dir_all(&dir).expect("folders");
    fs::write(
        dir.join(format!("rollout-2026-11-02T02-32-34-{CODEX_ID}.jsonl")),
        lines(&[
            json!({"type": "session_meta", "payload": {"id": CODEX_ID, "cwd": "/srv/project",
                "timestamp": "2026-11-02T01:32:34.000Z", "source": "cli"}}),
            json!({"type": "event_msg", "payload": {"type": "user_message", "message": "fix the retry flag"}}),
        ]),
    )
    .expect("a rollout");
    let events = crate::events::Hub::default();
    let sessions = Arc::new(crate::agents::sessions::Sessions::default());
    let listings = crate::agents::listings::Listings::new(
        sessions.clone(),
        crate::occupancy::Occupancy::over(
            crate::logs::claude::Store::at(nothing.path()),
            crate::logs::found::Stores::at(Some(codex.path()), None, None),
        ),
        Arc::default(),
        events.clone(),
        Arc::new(crate::agents::reviews::kept::Reviews::new(
            events,
            sessions.clone(),
        )),
        Arc::new(crate::agents::reopen::Reopen::new(
            sessions,
            Arc::new(crate::agents::family::links::Links::new(Default::default())),
        )),
    );

    let (entries, earlier) = listings
        .found_transcript(&SessionId::from(CODEX_ID), 10, None, None, true)
        .await
        .expect("a read")
        .expect("the session is found");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "fix the retry flag");
    assert_eq!(earlier, 0);
}
