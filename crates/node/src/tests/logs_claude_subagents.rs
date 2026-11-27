use super::*;
use std::io;
use crate::logs::claude::transcript::{self, Transcripts};
use crate::logs::window::Excerpt;
use beep_proto::node::agent::EntryKind;
use serde_json::json;

/// The entries of the transcript at `path` with their subagents.
fn entries(path: &Path, last: usize, before: Option<u64>, kept: &Transcripts) -> io::Result<Excerpt> {
    transcript::entries(path, last, before, kept).map(|page| attached(path, page))
}

/// A fabricated session that starts two subagents, an `Explore` one
/// and a custom `release-checker`, with their files beside it.
fn session(root: &Path) -> PathBuf {
    let project = root.join("projects/-srv-project");
    let subagents = project.join("11111111-1111-4111-8111-111111111111/subagents");
    fs::create_dir_all(&subagents).expect("folders");
    let lines = [
        json!({"type": "user", "message": {"role": "user", "content": "check the release"}}),
        json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "call_explore", "name": "Agent", "input": {
                "description": "Find the config loader", "subagent_type": "Explore", "model": "sonnet",
                "prompt": "Where is the config read?"}},
            {"type": "tool_use", "id": "call_release", "name": "Task", "input": {
                "description": "Check the release notes",
                "subagent_type": "release-checker", "model": "haiku", "prompt": "Compare the notes"}}
        ]}}),
        json!({"type": "user", "toolUseResult": {"agentId": "a2", "status": "completed"},
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_release", "content": "done"}
        ]}}),
        json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "call_explore", "content": "found"}
        ]}}),
        json!({"type": "assistant", "message": {"content": [
            {"type": "text", "text": "Both checks are done."}
        ]}}),
    ];
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    let path = project.join("11111111-1111-4111-8111-111111111111.jsonl");
    fs::write(&path, text).expect("the session");

    let explore = [
        json!({"type": "user", "isSidechain": true, "origin": {"kind": "agent"}, "message": {"role": "user",
            "content": "Where is the config read?"}}),
        json!({"type": "assistant", "isSidechain": true, "message": {"model": "claude-example", "content": [
            {"type": "tool_use", "id": "t1", "name": "Grep", "input": {"pattern": "load_config"}}
        ]}}),
        json!({"type": "user", "isSidechain": true, "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "src/config.rs"}
        ]}}),
        json!({"type": "assistant", "isSidechain": true, "message": {"model": "claude-example", "content": [
            {"type": "tool_use", "id": "t2", "name": "Read",
                "input": {"file_path": "/srv/project/src/config.rs"}}
        ]}}),
        json!({"type": "user", "isSidechain": true, "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t2", "is_error": true, "content": "no"}
        ]}}),
        json!({"type": "assistant", "isSidechain": true, "message": {"model": "claude-example", "content": [
            {"type": "text", "text": "It is read in src/config.rs."}
        ]}}),
    ];
    let explore: String = explore.iter().map(|line| format!("{line}\n")).collect();
    fs::write(subagents.join("agent-a1.jsonl"), explore).expect("a subagent");
    let meta = json!({"agentType": "Explore", "description": "Find the config loader",
        "toolUseId": "call_explore", "finished": true});
    fs::write(subagents.join("agent-a1.meta.json"), meta.to_string()).expect("its meta");

    // An older subagent: no meta, linked by the id its result named.
    let release = json!({"type": "assistant", "isSidechain": true, "message": {"content": [
        {"type": "text", "text": "The notes match the diff."}
    ]}});
    fs::write(subagents.join("agent-a2.jsonl"), format!("{release}\n")).expect("a subagent");
    path
}

#[test]
fn each_subagent_goes_on_the_call_that_started_it() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = session(root.path());
    let read = entries(&path, 10, None, &Transcripts::new(2)).expect("the transcript");
    let tools: Vec<&str> = read
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Tool)
        .map(|entry| entry.text.as_str())
        .collect();
    // The release check has no meta saying it finished, and its file
    // was just written, so it reads as working.
    assert_eq!(
        tools[0],
        "Find the config loader [completed]\n[steps] 2\n\
         [step completed] Grep: load_config\n\
         [step failed] Read: /srv/project/src/config.rs\n\
         [agent] Explore\n\
         [session] subagent:claude:11111111-1111-4111-8111-111111111111:a1\n\
         [model] claude-example\n\
         It is read in src/config.rs."
    );
    assert_eq!(
        tools[1],
        "Check the release notes [in_progress]\n[steps] 0\n\
         [agent] release-checker\n\
         [session] subagent:claude:11111111-1111-4111-8111-111111111111:a2\n\
         [model] haiku\n\
         The notes match the diff."
    );
}

#[test]
fn native_sidechain_messages_use_the_normal_transcript_reader() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = session(root.path());
    let store = Store::at(root.path());
    let id = "subagent:claude:11111111-1111-4111-8111-111111111111:a1";
    let found = super::session(&store, &SessionId::from(id))
        .expect("native metadata")
        .expect("child");
    assert_eq!(found.title.as_deref(), Some("Find the config loader"));
    let child = store.find(&SessionId::from(id)).expect("native file");
    let read = entries(&child, 50, None, &Transcripts::new(2)).expect("child transcript");
    assert_eq!(read.entries[0].text, "Where is the config read?");
    assert_eq!(
        read.entries.last().expect("reply").text,
        "It is read in src/config.rs."
    );
    assert!(
        read.entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Tool)
    );
    assert!(
        read.entries
            .iter()
            .any(|entry| entry.text.contains("[failed]\nno"))
    );
    assert!(
        read.entries
            .iter()
            .all(|entry| !entry.text.contains("Both checks are done"))
    );
    let mut live = vec![Entry::new(
        EntryKind::Tool,
        "Explore [completed]\n[steps] 0".to_owned(),
    )];
    attach_live(
        &path,
        &[(0, "call_explore".to_owned())],
        &mut live,
        &Transcripts::new(2),
    );
    assert!(
        live[0]
            .text
            .contains("[session] subagent:claude:11111111-1111-4111-8111-111111111111:a1")
    );
    assert!(
        store
            .find(&SessionId::from("subagent:claude:11111111-1111-4111-8111-111111111111:../a1"))
            .is_none()
    );
}

#[test]
fn a_subagent_before_the_entries_read_is_not_read() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = session(root.path());
    let read = entries(&path, 1, None, &Transcripts::new(2)).expect("the transcript");
    assert_eq!(read.entries.len(), 1);
    assert_eq!(read.entries[0].text, "Both checks are done.");
}

#[test]
fn a_correlated_subagent_keeps_running_until_its_meta_marks_it_finished() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = session(root.path());
    let transcript = path.with_extension("").join("subagents/agent-a2.jsonl");
    let old = SystemTime::now() - RUNNING - Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&transcript)
        .expect("the child file")
        .set_times(fs::FileTimes::new().set_modified(old))
        .expect("an old modification time");
    let mut agent = Found {
        id: "a2".into(),
        transcript,
        meta: serde_json::from_value(json!({"toolUseId": "call_release"}))
            .expect("unfinished meta"),
    };
    let spawn = Spawn::default();
    assert_eq!(status(&spawn, &agent), Status::InProgress);
    agent.meta.finished = Some(true);
    assert_eq!(status(&spawn, &agent), Status::Completed);
    agent.meta.stopped_by_user = Some(true);
    assert_eq!(status(&spawn, &agent), Status::Failed);
    agent.meta = Meta::default();
    assert_eq!(status(&spawn, &agent), Status::Completed);
}

#[test]
fn a_live_background_spawn_uses_its_childs_status_and_steps() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = session(root.path());
    let mut live = vec![
        Entry::new(
            EntryKind::Tool,
            "Check the release notes [completed]\n[steps] 0\nStarted in the background".into(),
        ),
        Entry::new(EntryKind::Tool, "Explore [completed]\n[steps] 0".into()),
    ];
    attach_live(
        &path,
        &[(0, "call_release".into()), (1, "call_explore".into())],
        &mut live,
        &Transcripts::new(2),
    );
    assert!(
        live[0]
            .text
            .starts_with("Check the release notes [in_progress]\n")
    );
    assert!(
        live[1]
            .text
            .starts_with("Find the config loader [completed]\n[steps] 2\n")
    );
    let meta = path.with_extension("").join("subagents/agent-a2.meta.json");
    fs::write(meta, r#"{"toolUseId":"call_release","finished":true}"#).expect("finished meta");
    attach_live(
        &path,
        &[(0, "call_release".into())],
        &mut live,
        &Transcripts::new(2),
    );
    assert!(
        live[0]
            .text
            .starts_with("Check the release notes [completed]\n")
    );
    assert_eq!(live[0].text.matches("[session]").count(), 1);
}
