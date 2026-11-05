use super::content::{replayed, thought_line};
use super::*;
use crate::agents::{
    compaction::COMPACTED_ENTRY,
    image_files,
    images::Picture,
    subagent::{Run, RunStep},
};
use agent_client_protocol::schema::v1::SessionUpdate;
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::node::agent::EntryKind;
use beep_proto::{AgentContext, image_key};
use futures::FutureExt;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

fn update(value: Value) -> SessionUpdate {
    serde_json::from_value(value).expect("a valid update")
}

fn live() -> Live {
    Live::new(
        SessionInfo {
            id: "a3f9c1".into(),
            cwd: "/srv/project".into(),
            ..SessionInfo::default()
        },
        Arc::default(),
        Arc::default(),
    )
}

fn thought(text: &str) -> SessionUpdate {
    update(json!({"sessionUpdate": "agent_thought_chunk",
        "content": {"type": "text", "text": text}}))
}

fn said(text: &str) -> SessionUpdate {
    update(json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": text}}))
}

#[test]
fn a_thought_is_an_entry_that_says_how_long_it_took() {
    let mut live = live();
    live.prompted("fix it");
    let start = Instant::now();
    live.apply_at(&thought("**Reading**\n\nThe test "), start);
    live.apply_at(&thought("expects the old text."), start);
    // Usage in the middle does not split it.
    live.apply_at(
        &update(json!({"sessionUpdate": "usage_update", "used": 10, "size": 100})),
        start + Duration::from_secs(3),
    );
    live.apply_at(&thought(" More."), start + Duration::from_secs(5));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries[1].kind, EntryKind::Thought);
    assert_eq!(entries[1].ms, None, "not over yet");
    live.apply_at(&said("Fixed."), start + Duration::from_millis(12_400));
    live.apply_at(&thought("Now the docs.\n"), start + Duration::from_secs(13));
    live.end_thought(start + Duration::from_secs(15));
    let (entries, _) = live.transcript(10);
    let kinds: Vec<_> = entries.iter().map(|entry| entry.kind).collect();
    assert_eq!(
        kinds,
        [
            EntryKind::User,
            EntryKind::Thought,
            EntryKind::Agent,
            EntryKind::Thought
        ]
    );
    assert_eq!(
        entries[1].text,
        "**Reading**\n\nThe test expects the old text. More."
    );
    assert_eq!(entries[1].ms, Some(12_400));
    assert_eq!(entries[2].text, "Fixed.");
    assert_eq!(entries[3].text, "Now the docs.");
    // It started when the text before it arrived.
    assert_eq!(entries[3].ms, Some(2_600));
}

#[test]
fn real_agents_thought_chunks_join_into_one_entry() {
    let mut live = live();
    live.prompted("is 391 prime?");
    let start = Instant::now();
    // Codex sends a summary's parts with a blank line before each.
    for chunk in ["\n\n", "**Computing 17 × 23**", "\n\n", "**Checking**"] {
        live.apply_at(&thought(chunk), start);
    }
    live.apply_at(&said("Not prime."), start + Duration::from_secs(2));
    // Claude Code's summarized thinking arrives cut mid-word.
    for chunk in ["I'm checking wh", "ether 391 is pr", "ime by factoring it."] {
        live.apply_at(&thought(chunk), start + Duration::from_secs(3));
    }
    live.apply_at(
        &update(json!({"sessionUpdate": "tool_call", "toolCallId": "t1",
            "title": "Read notes.md", "kind": "read", "status": "pending"})),
        start + Duration::from_secs(4),
    );
    let (entries, _) = live.transcript(10);
    let shown: Vec<_> = entries
        .iter()
        .map(|entry| (entry.kind, entry.text.as_str(), entry.ms))
        .collect();
    assert_eq!(
        shown[..4],
        [
            (EntryKind::User, "is 391 prime?", None),
            (
                EntryKind::Thought,
                "**Computing 17 × 23**\n\n**Checking**",
                Some(2000)
            ),
            (EntryKind::Agent, "Not prime.", None),
            (
                EntryKind::Thought,
                "I'm checking whether 391 is prime by factoring it.",
                Some(2000)
            ),
        ]
    );
}

#[test]
fn a_loaded_session_replays_its_prompts_and_answers() {
    let mut live = live();
    let chunk = |kind: &str, text: &str| {
        update(json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}}))
    };
    live.apply(&chunk("user_message_chunk", "Fix the "));
    live.apply(&chunk("user_message_chunk", "build"));
    live.apply(&chunk("agent_message_chunk", "Fixed."));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].kind, EntryKind::User);
    assert_eq!(entries[0].text, "Fix the build");
    assert_eq!(entries[1].text, "Fixed.");
    // Once running, prompts come from `prompted`, not the agent's echo.
    live.prompted("again");
    live.apply(&chunk("user_message_chunk", "again"));
    assert_eq!(live.transcript(10).0.len(), 3);
}

#[test]
fn a_replayed_image_becomes_an_image_line() {
    let mut live = live();
    let data = STANDARD.encode(b"\x89PNG");
    let line = format!("[image: image-{}]", image_key(b"\x89PNG"));
    live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
        "content": {"type": "text", "text": format!("what is this[@image](data:image/png;base64,{data})")}})));
    live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
        "content": {"type": "image", "mimeType": "image/png", "data": data}})));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries[0].text, format!("what is this\n{line}\n{line}"));
    let mut found = Vec::new();
    assert_eq!(
        replayed("a [link](https://x.dev) stays", &mut found),
        "a [link](https://x.dev) stays"
    );
    assert!(found.is_empty());
}

#[test]
fn tool_calls_go_with_their_entries() {
    let mut live = live();
    for n in 0..MAX_ENTRIES {
        live.prompted("again");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": format!("t{n}"),
            "title": "Run the tests",
            "kind": "execute",
            "status": "completed",
        })));
    }
    live.prompted("once more");
    assert!(
        live.tools.len() <= MAX_ENTRIES / 2 + 1,
        "{}",
        live.tools.len()
    );
}

#[test]
fn a_tool_result_image_shows_on_its_tool_and_goes_to_the_server() {
    let mut live = live();
    live.prompted("take a screenshot");
    let png = b"\x89PNG\r\n\x1a\nscreen".to_vec();
    live.apply(&update(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "t1",
        "title": "Screenshot",
        "kind": "other",
        "status": "completed",
        "content": [
            {"type": "content", "content": {"type": "text", "text": "taken"}},
            {"type": "content", "content": {"type": "image", "mimeType": "image/png",
                "data": STANDARD.encode(&png)}}
        ]
    })));
    let line = format!("[image: image-{}]", image_key(&png));
    let (entries, _) = live.transcript(2);
    assert_eq!(
        entries[0].text,
        format!("Screenshot [completed]\ntaken\n{line}")
    );
    assert_eq!(entries[1].kind, EntryKind::Agent);
    assert_eq!(
        entries[1].text,
        format!("{line}\n"),
        "shown with the tools folded"
    );
    assert!(live.picture(&image_key(&png)).is_some());
    let sent = live.images.next_unsent().now_or_never().expect("one waits");
    assert_eq!(sent.session, "a3f9c1");
    assert_eq!(sent.picture.data, png);
    // The same picture again is not sent twice.
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "t1",
        "content": [{"type": "content", "content": {"type": "image", "mimeType": "image/png",
            "data": STANDARD.encode(&png)}}]
    })));
    assert!(live.images.next_unsent().now_or_never().is_none());
}

#[test]
fn a_turn_names_the_image_files_it_mentioned_and_wrote() {
    let mut live = live();
    live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "Earlier: old.png"}})));
    live.prompted("plot it");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "w1",
        "title": "Write plots/b.svg",
        "kind": "edit",
        "status": "completed",
        "locations": [{"path": "/srv/project/plots/b.svg"}]
    })));
    live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "Saved `plots/a.png` and plots/b.svg."}})));
    let files = live.turn_files().expect("files named");
    assert_eq!(files.cwd, Path::new("/srv/project"));
    assert_eq!(
        files.paths,
        [
            PathBuf::from("/srv/project/plots/a.png"),
            PathBuf::from("/srv/project/plots/b.svg")
        ]
    );
    assert_eq!(files.entry, Some(3));
    let png = b"\x89PNG\r\n\x1a\nplot".to_vec();
    let picture = Picture {
        mime: "image/png".into(),
        data: png.clone(),
    };
    live.show_files(files.entry, vec![picture.clone(), picture]);
    let line = format!("[image: image-{}]", image_key(&png));
    let (entries, _) = live.transcript(1);
    assert_eq!(
        entries[0].text,
        format!("Saved `plots/a.png` and plots/b.svg.\n{line}"),
        "once"
    );
    live.prompted("nothing");
    assert_eq!(live.turn_files(), None);
}

/// A Read tool call of `path`, as Claude Code's ACP adapter sends it.
fn read_call(id: &str, path: &str) -> SessionUpdate {
    update(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": id,
        "title": format!("Read {path}"),
        "kind": "read",
        "status": "pending",
        "locations": [{"path": path, "line": 1}],
        "rawInput": {"file_path": path}
    }))
}

/// Its result: `png` as an image block, or plain text without one.
fn read_done(id: &str, png: Option<&[u8]>) -> SessionUpdate {
    let content = match png {
        Some(png) => json!({"type": "image", "mimeType": "image/png",
            "data": STANDARD.encode(png)}),
        None => json!({"type": "text", "text": "(binary)"}),
    };
    update(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": id,
        "status": "completed",
        "content": [{"type": "content", "content": content}]
    }))
}

fn agent_lines(live: &Live) -> Vec<String> {
    live.transcript(MAX_ENTRIES)
        .0
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::Agent)
        .map(|entry| entry.text)
        .collect()
}

#[test]
fn an_image_file_a_read_named_shows_when_inside_the_folder() {
    let root = std::env::temp_dir().join(format!("beep-live-reads-{}", std::process::id()));
    let cwd = root.join("project");
    std::fs::create_dir_all(cwd.join("sketches")).unwrap();
    let png = b"\x89PNG\r\n\x1a\nlineup".to_vec();
    std::fs::write(cwd.join("sketches/lineup.png"), &png).unwrap();
    std::fs::write(root.join("outside.png"), b"\x89PNG\r\n\x1a\nout").unwrap();
    let mut live = Live::new(
        SessionInfo {
            id: "b7e2d0".into(),
            cwd: cwd.display().to_string(),
            ..SessionInfo::default()
        },
        Arc::default(),
        Arc::default(),
    );
    live.prompted("look at them");
    let inside = cwd.join("sketches/lineup.png").display().to_string();
    let outside = root.join("outside.png").display().to_string();
    live.apply(&read_call("r1", &inside));
    live.apply(&read_done("r1", None));
    live.apply(&read_call("r2", &outside));
    live.apply(&read_done("r2", None));
    // Still running: its file is not read.
    live.apply(&read_call("r3", "notes.png"));
    live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "Looked."}})));
    let files = live.turn_files().expect("files named");
    let pictures = image_files::read(&files.cwd, &files.paths);
    assert_eq!(pictures.len(), 1, "only the one inside the folder");
    assert_eq!(pictures[0].data, png);
    live.show_files(files.entry, pictures);
    let line = format!("[image: image-{}]", image_key(&png));
    assert_eq!(agent_lines(&live), [format!("Looked.\n{line}")]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_turn_shows_each_tool_image_once_and_a_bounded_few() {
    let mut live = live();
    live.prompted("look twice");
    let png = b"\x89PNG\r\n\x1a\nsame".to_vec();
    live.apply(&read_call("r1", "/srv/project/a.png"));
    live.apply(&read_done("r1", Some(&png)));
    live.apply(&read_call("r2", "/srv/project/b.png"));
    live.apply(&read_done("r2", Some(&png)));
    let line = format!("[image: image-{}]", image_key(&png));
    assert_eq!(agent_lines(&live), [format!("{line}\n")]);
    // Nor again once the turn's files are read.
    let picture = Picture {
        mime: "image/png".into(),
        data: png.clone(),
    };
    live.show_files(None, vec![picture]);
    assert_eq!(agent_lines(&live).len(), 1);
    // The next turn may show as many as the cap.
    live.prompted("look at many");
    for n in 0..image_files::MAX_PER_TURN + 2 {
        let mut png = png.clone();
        png.push(u8::try_from(n).expect("a few"));
        let id = format!("m{n}");
        live.apply(&read_call(&id, &format!("/srv/project/{n}.png")));
        live.apply(&read_done(&id, Some(&png)));
    }
    assert_eq!(agent_lines(&live).len(), 1 + image_files::MAX_PER_TURN);
}

#[tokio::test]
async fn a_running_cost_is_counted_per_turn() {
    let usage = Arc::new(UsageHub::default());
    let mut live = Live::new(
        SessionInfo {
            id: "a3f9c1".into(),
            agent: "opencode".into(),
            cwd: "/srv/project".into(),
            ..SessionInfo::default()
        },
        Arc::default(),
        usage.clone(),
    );
    let cost = |amount: f64| {
        update(
            json!({"sessionUpdate": "usage_update", "used": 9, "size": 10,
            "cost": {"amount": amount, "currency": "USD"}}),
        )
    };
    live.apply(&cost(0.5));
    live.apply(&cost(1.25));
    live.spent(TurnTokens {
        total: 900,
        models: Vec::new(),
    });
    let first = usage.next_turn().await;
    assert_eq!(first.cost_usd, Some(1.25));
    assert_eq!((first.tokens, first.agent.as_str()), (900, "opencode"));
    live.apply(&cost(1.5));
    live.spent(TurnTokens::default());
    assert_eq!(usage.next_turn().await.cost_usd, Some(0.25));
    // A turn that spent nothing is not counted.
    live.spent(TurnTokens::default());
    live.spent(TurnTokens {
        total: 5,
        models: Vec::new(),
    });
    let last = usage.next_turn().await;
    assert_eq!((last.tokens, last.cost_usd), (5, None));
    assert_eq!((live.info.tokens, live.info.cost_usd), (905, Some(1.5)));
}

#[test]
fn usage_commands_and_compactions_are_kept() {
    let mut live = live();
    assert!(!live.apply(&update(
        json!({"sessionUpdate": "usage_update", "used": 900, "size": 1000})
    )));
    assert_eq!(
        live.info.context,
        Some(AgentContext {
            used: 900,
            size: 1000
        })
    );
    live.apply(&update(
        json!({"sessionUpdate": "available_commands_update",
        "availableCommands": [{"name": "compact", "description": "Compact"}]}),
    ));
    assert_eq!(live.commands.len(), 1);
    assert_eq!(live.commands[0].name, "compact");
    assert_eq!(live.commands[0].description, "Compact");
    let compaction = |status: &str| {
        update(
            json!({"sessionUpdate": "compaction_update", "compactionId": "c1", "status": status}),
        )
    };
    assert!(live.apply(&compaction("in_progress")));
    live.apply(&compaction("completed"));
    live.apply(&compaction("completed"));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 1, "shown once");
    assert_eq!(entries[0].kind, EntryKind::Other);
    assert_eq!(entries[0].text, COMPACTED_ENTRY);
    assert_eq!(live.info.context, None, "the usage before no longer holds");
    assert!(live.take_compacted());
    assert!(!live.take_compacted());
}

#[test]
fn cutting_keeps_characters_whole() {
    assert_eq!(cut("héllo", 2), "h");
    assert_eq!(cut("hi", 10), "hi");
}

#[test]
fn a_thought_shows_its_heading_or_first_whole_sentence() {
    assert_eq!(
        thought_line("**Planning the fix**\n\nI will"),
        Some("Planning the fix".into())
    );
    assert_eq!(
        thought_line("**Reading**\n\nok. **Running the tests** now"),
        Some("Running the tests".into())
    );
    assert_eq!(
        thought_line("Let me check `src/main.rs`. Then"),
        Some("Let me check src/main.rs.".into())
    );
    assert_eq!(thought_line("Looking at e.g"), None, "not whole yet");
    assert_eq!(thought_line("**Half"), None);
    assert_eq!(
        thought_line("First line\nsecond"),
        Some("First line".into())
    );
}

#[test]
fn a_subagents_calls_go_on_the_call_that_started_it() {
    let mut live = live();
    live.prompted("look around");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "task", "title": "Explore the code",
        "status": "in_progress", "_meta": {"claudeCode": {"toolName": "Agent"}}
    })));
    let inner = |id: &str, title: &str| {
        update(json!({
            "sessionUpdate": "tool_call", "toolCallId": id, "title": title, "status": "in_progress",
            "_meta": {"claudeCode": {"toolName": "Read", "parentToolUseId": "task"}}
        }))
    };
    live.apply(&inner("r1", "Read src/main.rs"));
    live.apply(&inner("r2", "grep -n todo"));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "r1", "status": "completed",
        "_meta": {"claudeCode": {"parentToolUseId": "task"}}
    })));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "task", "status": "completed",
        "content": [{"type": "content", "content": {"type": "text", "text": "Found it"}}]
    })));
    let (entries, _) = live.transcript(10);
    assert_eq!(
        entries.len(),
        2,
        "the subagent's calls are not entries of their own"
    );
    assert_eq!(
        entries[1].text,
        "Explore the code [completed]\n[steps] 2\n[step completed] Read src/main.rs\n[step in_progress] grep -n todo\nFound it"
    );
}

#[test]
fn a_codex_subagent_shows_on_its_spawn_and_takes_its_rollouts_steps() {
    let mut live = live();
    live.prompted("spawn a subagent that sleeps");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "spawn", "title": "spawnAgent",
        "kind": "other", "status": "in_progress",
        "rawInput": {"prompt": "Sleep for 60 seconds", "senderThreadId": "root",
            "receiverThreadIds": [], "agentsStates": {}}
    })));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "spawn", "title": "spawnAgent",
        "status": "completed",
        "rawInput": {"prompt": "Sleep for 60 seconds", "senderThreadId": "root",
            "receiverThreadIds": ["t1"], "agentsStates": {"t1": {"status": "running"}}}
    })));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "act1", "title": "Start subagent sleep_test",
        "kind": "other", "status": "in_progress",
        "rawInput": {"agentThreadId": "t1", "agentPath": "/root/sleep_test",
            "activityKind": "started"}
    })));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 2, "the activity is not an entry of its own");
    assert_eq!(entries[1].text, "sleep_test [in_progress]\n[steps] 0");
    let children = live.codex_children(0);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].threads, ["t1"]);

    let run = Run {
        steps: vec![RunStep {
            title: "sleep".into(),
            done: true,
        }],
        count: 1,
        result: Some("Slept for a minute.".into()),
        working: false,
        name: Some("Kepler".into()),
        task: Some("sleep_test".into()),
    };
    let (at, text) = live.with_run(&children[0].call, &run).expect("its entry");
    assert_eq!(at, 1);
    assert_eq!(
        text,
        "sleep_test [completed]\n[steps] 1\n[step completed] sleep\n[agent] Kepler\nSlept for a minute."
    );

    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "act2", "title": "Complete subagent sleep_test",
        "kind": "other", "status": "completed",
        "rawInput": {"agentThreadId": "t1", "agentPath": "/root/sleep_test",
            "activityKind": "completed"}
    })));
    assert_eq!(
        live.transcript(10).0[1].text,
        "sleep_test [completed]\n[steps] 0"
    );
}

#[test]
fn a_session_looks_up_its_place_when_a_call_changes_the_folder() {
    let mut live = live();
    assert_eq!(
        live.place_wanted.take().as_deref(),
        Some(std::path::Path::new("/srv/project")),
        "the folder it started in"
    );
    // A read names a file, not where the agent works.
    live.apply(&read_call("r1", "/srv/other/a.rs"));
    assert_eq!(live.place_wanted, None);
    // A call that names a working folder moves it, once.
    let exec = |id: &str, workdir: &str| {
        update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": "exec_command",
            "kind": "execute",
            "status": "pending",
            "rawInput": {"cmd": "ls", "workdir": workdir}
        }))
    };
    live.apply(&exec("e1", "/srv/project/.worktrees/fix"));
    assert_eq!(
        live.place_wanted.take().as_deref(),
        Some(std::path::Path::new("/srv/project/.worktrees/fix"))
    );
    live.apply(&exec("e2", "/srv/project/.worktrees/fix"));
    assert_eq!(live.place_wanted, None, "the same folder again");
    // An edit counts for the folder of its file.
    live.apply(&update(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "w1",
        "title": "Edit lib.rs",
        "kind": "edit",
        "status": "pending",
        "locations": [{"path": "/srv/project/src/lib.rs"}]
    })));
    assert_eq!(
        live.place_wanted.take().as_deref(),
        Some(std::path::Path::new("/srv/project/src"))
    );
}

#[test]
fn a_session_the_agent_has_not_named_takes_its_first_prompt_line() {
    let mut live = live();
    assert_eq!(live.info.title, None, "no prompt yet");
    live.prompted("\n  Fix the flaky test  \nit fails on CI");
    assert_eq!(live.info.title.as_deref(), Some("Fix the flaky test"));
    live.prompted("something else");
    assert_eq!(live.info.title.as_deref(), Some("Fix the flaky test"));
    live.apply_at(
        &update(json!({"sessionUpdate": "session_info_update", "title": "Flaky CI"})),
        Instant::now(),
    );
    assert_eq!(live.info.title.as_deref(), Some("Flaky CI"));
    live.prompted("and more");
    assert_eq!(live.info.title.as_deref(), Some("Flaky CI"));
}
