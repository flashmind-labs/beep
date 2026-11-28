use super::content::{replayed, thought_line};
use super::limits::*;
use super::*;
use crate::agents::{compaction::COMPACTED_ENTRY, image_files};
use crate::logs::picture::Picture;
use crate::logs::subagent::{Run, RunStep};
use crate::text::cut;
use agent_client_protocol::v1::schema::SessionUpdate;
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::node::agent::{EntryKind, Status};
use beep_proto::watched;
use beep_proto::{AgentContext, image_key};
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

fn update(value: Value) -> SessionUpdate {
    serde_json::from_value(value).expect("a valid update")
}

fn live_titled(title: &str) -> Live {
    Live::new(
        SessionInfo {
            id: "a3f9c1".into(),
            cwd: "/srv/project".into(),
            title: Some(title.into()),
            ..SessionInfo::default()
        },
        Arc::default(),
        Arc::default(),
    )
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
fn interruption_replay_stays_apart_from_the_next_user_prompt() {
    let chunk = |kind: &str, text: &str| {
        update(json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}}))
    };
    for notice in [
        "[Request interrupted by user]",
        "[Request interrupted by user for tool use]",
    ] {
        for joined in [false, true] {
            let mut live = live_titled("A conversation");
            live.info.state.status = Status::Starting;
            if joined {
                live.apply(&chunk(
                    "user_message_chunk",
                    &format!("{notice}Use the other machine"),
                ));
            } else {
                live.apply(&chunk("user_message_chunk", notice));
                live.apply(&chunk("user_message_chunk", "Use the other machine"));
            }
            let (entries, _) = live.transcript(10);
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].kind, EntryKind::Other);
            assert_eq!(entries[0].text, "Interrupted");
            assert_eq!(entries[1].kind, EntryKind::User);
            assert_eq!(entries[1].text, "Use the other machine");
        }
    }
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
    assert_eq!(entries[0].text, format!("what is this\n{line}\n{line}\n"));
    let mut found = Vec::new();
    assert_eq!(
        replayed("a [link](https://x.dev) stays", &mut found),
        "a [link](https://x.dev) stays"
    );
    assert!(found.is_empty());
}

#[test]
fn replayed_image_chunks_stay_apart_from_the_text_around_them() {
    let mut live = live();
    let png = b"\x89PNG\r\n\x1a\nshot";
    let data = STANDARD.encode(png);
    for text in [
        "Look at this".to_owned(),
        format!("[@image](data:image/png;base64,{data})"),
        "and this too".to_owned(),
    ] {
        live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": text}})));
    }
    live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
        "content": {"type": "image", "mimeType": "image/png", "data": data}})));
    live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
        "content": {"type": "text", "text": "More detail"}})));
    let line = format!("[image: image-{}]", image_key(png));
    assert_eq!(
        live.transcript(10).0[0].text,
        format!("Look at this\n{line}\nand this too\n{line}\nMore detail")
    );
    assert_eq!(live.picture(&image_key(png)).unwrap().data, png);
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
fn a_long_turn_releases_tool_records_with_discarded_entries() {
    let mut live = live();
    live.prompted("keep working");

    for n in 0..MAX_ENTRIES + 128 {
        let id = format!("t{n}");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": id,
            "title": "Read a file", "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "file contents"}}]
        })));
        live.codex_threads
            .insert(format!("thread{n}"), ToolCallId::new(id));
    }

    assert_eq!(live.transcript.len(), MAX_ENTRIES);
    assert!(live.tools.len() <= MAX_ENTRIES + 63);
    assert!(live.codex_threads.len() <= MAX_ENTRIES + 63);
    assert!(!live.tools.contains_key(&ToolCallId::new("t0")));

    let last = format!("t{}", MAX_ENTRIES + 127);
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": last,
        "content": [{"type": "content", "content": {"type": "text", "text": "still updates"}}]
    })));
    assert!(live.transcript(1).0[0].text.contains("still updates"));
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
}

#[test]
fn a_turn_the_agent_starts_on_its_own_is_a_new_message() {
    let mut live = live();
    let chunk = |id: &str, text: &str| {
        update(
            json!({"sessionUpdate": "agent_message_chunk", "messageId": id,
            "content": {"type": "text", "text": text}}),
        )
    };
    live.prompted("watch it");
    live.apply(&chunk("msg_1", "Still "));
    live.apply(&chunk("msg_1", "running."));
    // A background task woke the agent: no prompt, a new message.
    live.apply(&chunk("msg_2", "Done."));

    let (entries, _) = live.transcript(10);
    let said: Vec<&str> = entries.iter().map(|entry| entry.text.as_str()).collect();
    assert_eq!(said, ["watch it", "Still running.", "Done."]);
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
    let turns = usage.turns.follow(iroh::SecretKey::generate().public());
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
    assert_eq!(live.info.cost_usd, Some(1.25));
    live.spent(TurnTokens {
        total: 900,
        models: Vec::new(),
    });
    let first = next_turn(&turns).await;
    assert_eq!(first.cost_usd, Some(1.25));
    assert_eq!((first.tokens, first.agent.as_ref()), (900, "opencode"));
    live.apply(&cost(1.5));
    live.spent(TurnTokens::default());
    assert_eq!(next_turn(&turns).await.cost_usd, Some(0.25));
    // A turn that spent nothing is not counted.
    live.spent(TurnTokens::default());
    live.spent(TurnTokens {
        total: 5,
        models: Vec::new(),
    });
    let last = next_turn(&turns).await;
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
    assert_eq!(entries[0].kind, EntryKind::Compacted);
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

/// A background subagent keeps calling tools while the agent writes; its
/// calls add no entry, so the message stays one entry.
#[test]
fn a_subagents_call_does_not_split_the_message_being_written() {
    let mut live = live();
    live.prompted("look around");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "task", "title": "Explore the code",
        "status": "in_progress", "_meta": {"claudeCode": {"toolName": "Agent"}}
    })));
    live.apply(&said("**Renderers,\n"));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "r1", "title": "Read src/main.rs",
        "status": "in_progress",
        "_meta": {"claudeCode": {"toolName": "Read", "parentToolUseId": "task"}}
    })));
    live.apply(&said("not arbitrary elements**"));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2].text, "**Renderers,\nnot arbitrary elements**");
}

/// Claude Code stamps no parent on the calls after an inline skill, only on
/// those of a skill that forks (`context: fork`), which runs as a subagent
/// and is titled as one.
#[test]
fn only_a_forked_skill_takes_the_calls_after_it() {
    let skill = |id: &str| {
        update(json!({
            "sessionUpdate": "tool_call", "toolCallId": id, "title": "Load skill: demo",
            "kind": "other", "status": "in_progress", "rawInput": {"skill": "demo"},
            "_meta": {"claudeCode": {"toolName": "Skill"}}
        }))
    };
    let bash = |parent: Option<&str>| {
        let mut meta = json!({"toolName": "Bash"});
        if let Some(parent) = parent {
            meta["parentToolUseId"] = json!(parent);
        }
        update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "bash", "title": "echo step",
            "kind": "execute", "status": "in_progress", "_meta": {"claudeCode": meta}
        }))
    };

    let mut inline = live();
    inline.prompted("run the demo");
    inline.apply(&skill("load"));
    inline.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "load", "status": "completed"
    })));
    inline.apply(&bash(None));
    let (entries, _) = inline.transcript(10);
    assert_eq!(entries.len(), 3, "the next call is a step of the turn");
    assert_eq!(entries[1].text, "Load skill: demo [completed]");

    let mut forked = live();
    forked.prompted("run the demo");
    forked.apply(&skill("fork"));
    forked.apply(&bash(Some("fork")));
    let (entries, _) = forked.transcript(10);
    assert_eq!(entries.len(), 2, "the fork's call is its step");
    assert_eq!(
        entries[1].text,
        "Skill: demo (subagent) [in_progress]\n[steps] 1\n[step in_progress] echo step"
    );

    let mut background = live();
    background.prompted("run the demo");
    background.apply(&skill("bg"));
    background.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "bg", "status": "completed",
        "content": [{"type": "content", "content": {"type": "text",
            "text": "Skill \"demo\" launched (forked execution, running in the background)"}}]
    })));
    let (entries, _) = background.transcript(10);
    assert!(
        entries[1]
            .text
            .starts_with("Skill: demo (subagent) [completed]"),
        "{}",
        entries[1].text
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
        session: None,
        steps: vec![RunStep {
            title: "sleep".into(),
            done: true,
        }],
        count: 1,
        result: Some("Slept for a minute.".into()),
        working: false,
        name: Some("Kepler".into()),
        model: Some("child-model".into()),
        task: Some("sleep_test".into()),
    };
    let (at, text) = live.with_run(&children[0].call, &run).expect("its entry");
    assert_eq!(at, 1);
    assert_eq!(
        text,
        "sleep_test [completed]\n[steps] 1\n[step completed] sleep\n[agent] Kepler\n[model] child-model\nSlept for a minute."
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
fn an_idle_session_runs_until_its_subagents_end() {
    let mut live = live();
    live.prompted("spawn a subagent that sleeps");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "spawn", "title": "spawnAgent",
        "kind": "other", "status": "completed",
        "rawInput": {"prompt": "Sleep for 60 seconds", "senderThreadId": "root",
            "receiverThreadIds": ["t1"], "agentsStates": {"t1": {"status": "running"}}}
    })));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "read", "title": "Read README.md",
        "kind": "read", "status": "completed"
    })));
    live.info.state.status = Status::Idle;
    assert_eq!(live.info.state.subagents, 1);
    assert!(live.info.state.runs(), "its subagent still runs");

    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "act1", "title": "Complete subagent sleep_test",
        "kind": "other", "status": "completed",
        "rawInput": {"agentThreadId": "t1", "agentPath": "/root/sleep_test",
            "activityKind": "completed"}
    })));
    assert_eq!(live.info.state.subagents, 0);
    assert!(!live.info.state.runs());
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
fn pages_before_an_entry_cover_a_live_transcript_without_a_gap_or_a_repeat() {
    let mut session = live();

    for n in 0..25 {
        session.prompted(&format!("prompt {n}"));
        session.push(EntryKind::Agent, &format!("answer {n}"));
    }

    let (all, first) = session.transcript(MAX_ENTRIES);
    assert_eq!((all.len(), first), (50, 0));

    let mut seen = Vec::new();
    let mut before = None;

    loop {
        let (page, earlier) = session.transcript_before(8, before);

        if let Some(end) = before {
            assert_eq!(earlier + page.len() as u64, end);
        }

        seen.splice(0..0, page);

        if earlier == 0 {
            break;
        }
        before = Some(earlier);
    }

    assert_eq!(seen, all);
}

#[test]
fn a_page_before_what_the_session_dropped_is_empty_and_says_where_the_rest_starts() {
    let mut session = live();

    for n in 0..MAX_ENTRIES + 10 {
        session.push(EntryKind::Agent, &format!("answer {n}"));
    }

    let (page, earlier) = session.transcript_before(5, Some(4));
    assert!(page.is_empty());
    assert_eq!(earlier, 10);
    let (page, earlier) = session.transcript_before(5, Some(12));
    assert_eq!(earlier, 10);
    assert_eq!(page.len(), 2);
}

#[test]
fn a_resumed_session_takes_its_first_replayed_prompt_line_as_title() {
    let chunk = |kind: &str, text: &str| {
        update(json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}}))
    };
    let mut first = live();
    first.apply(&chunk("user_message_chunk", "Fix the "));
    assert_eq!(first.info.title.as_deref(), Some("Fix the"));
    first.apply(&chunk("user_message_chunk", "build\nit fails"));
    assert_eq!(first.info.title.as_deref(), Some("Fix the build"));
    first.apply(&chunk("agent_message_chunk", "Fixed."));
    first.apply(&chunk("user_message_chunk", "now the docs"));
    assert_eq!(
        first.info.title.as_deref(),
        Some("Fix the build"),
        "the first only"
    );

    // A title the session already has stays.
    let mut named = live_titled("Saved name");
    named.apply(&chunk("user_message_chunk", "Fix the build"));
    assert_eq!(named.info.title.as_deref(), Some("Saved name"));

    // So does one the agent gives while the prompt replays.
    let mut second = live();
    second.apply(&chunk("user_message_chunk", "Fix "));
    second.apply_at(
        &update(json!({"sessionUpdate": "session_info_update", "title": "Flaky CI"})),
        Instant::now(),
    );
    second.apply(&chunk("user_message_chunk", "the build"));
    assert_eq!(second.info.title.as_deref(), Some("Flaky CI"));
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

#[test]
fn steering_keeps_the_running_turns_files_and_start() {
    let mut live = live();
    live.prompted("make an image");
    live.turn_links.push("result.png".into());
    let start = live.turn_start;
    let began = live.turn_began;
    live.steered("make it blue");
    assert_eq!(live.turn_start, start);
    assert_eq!(live.turn_began, began);
    assert_eq!(live.turn_links, ["result.png"]);
    assert_eq!(live.transcript(10).0.last().unwrap().text, "make it blue");
}

#[test]
fn flsh_credit_snapshots_preserve_context_and_do_not_count_as_session_cost() {
    let usage = Arc::new(UsageHub::default());
    let mut live = Live::new(
        SessionInfo {
            agent: "flsh".into(),
            ..SessionInfo::default()
        },
        Arc::default(),
        usage.clone(),
    );
    live.apply(&update(
        json!({"sessionUpdate": "usage_update", "used": 123, "size": 1000}),
    ));
    let balance = |remaining: Option<&str>| {
        update(json!({
            "sessionUpdate": "usage_update", "used": 0, "size": 1000,
            "_meta": {"flsh/contextUsageAvailable": false, "flsh/credits": {
                "provider": "openrouter", "consumed": "0.1",
                "balance": {"scope": "api_key", "unit": "usd", "remaining": remaining,
                    "total": "10", "used": "2.75", "resets": "monthly"}
            }}
        }))
    };
    live.apply(&balance(Some("7.25")));
    assert_eq!(live.info.context.as_ref().unwrap().used, 123);
    assert_eq!(live.info.credits[0].remaining.as_deref(), Some("7.25"));
    assert_eq!(live.info.cost_usd, None);
    assert_eq!(live.unbilled, None);
    let plans = usage.plans();
    assert_eq!(
        watched::read(&plans, |plans| plans
            [&(beep_proto::AgentSlug::new("flsh"), None)]
            .credits[0]
            .remaining
            .clone())
        .as_deref(),
        Some("7.25")
    );
    live.apply(&balance(Some("0")));
    assert_eq!(
        watched::read(&plans, |plans| plans
            [&(beep_proto::AgentSlug::new("flsh"), None)]
            .credits
            .len()),
        1
    );
    assert_eq!(
        watched::read(&plans, |plans| plans
            [&(beep_proto::AgentSlug::new("flsh"), None)]
            .credits[0]
            .remaining
            .clone())
        .as_deref(),
        Some("0")
    );
    live.apply(&balance(None));
    assert_eq!(
        watched::read(&plans, |plans| plans
            [&(beep_proto::AgentSlug::new("flsh"), None)]
            .credits[0]
            .remaining
            .clone()),
        None
    );
    live.apply(&balance(Some("NaN")));
    assert_eq!(
        watched::read(&plans, |plans| plans
            [&(beep_proto::AgentSlug::new("flsh"), None)]
            .credits[0]
            .remaining
            .clone()),
        None
    );
}

#[tokio::test]
async fn flsh_keeps_counting_priced_calls_after_a_call_without_a_price() {
    let usage = Arc::new(UsageHub::default());
    let turns = usage.turns.follow(iroh::SecretKey::generate().public());
    let mut live = Live::new(
        SessionInfo {
            agent: "flsh".into(),
            ..SessionInfo::default()
        },
        Arc::default(),
        usage.clone(),
    );
    let subtotal = |amount: &str| {
        update(json!({
            "sessionUpdate": "usage_update", "used": 10, "size": 1000,
            "_meta": {"flsh/sessionUsage": {"total_cost": amount, "cost_unknown": true}}
        }))
    };

    live.apply(&update(json!({
        "sessionUpdate": "usage_update", "used": 10, "size": 1000,
        "cost": {"amount": 0.25, "currency": "USD"},
        "_meta": {"flsh/sessionUsage": {"total_cost": "0.25", "cost_unknown": false}}
    })));
    live.spent(TurnTokens::default());
    assert_eq!(next_turn(&turns).await.cost_usd, Some(0.25));

    live.apply(&subtotal("0.25"));
    live.apply(&subtotal("1.75"));
    live.apply(&subtotal("1.75"));
    live.apply(&subtotal("NaN"));
    assert_eq!(live.info.cost_usd, Some(1.75));

    live.spent(TurnTokens::default());
    assert_eq!(next_turn(&turns).await.cost_usd, Some(1.50));
    assert_eq!(live.info.cost_usd, Some(1.75));
}

#[tokio::test]
async fn flsh_session_tokens_are_counted_once_across_repeated_usage_updates() {
    let usage = Arc::new(UsageHub::default());
    let turns = usage.turns.follow(iroh::SecretKey::generate().public());
    let mut live = Live::new(
        SessionInfo {
            agent: "flsh".into(),
            ..SessionInfo::default()
        },
        Arc::default(),
        usage.clone(),
    );
    let totals = |input, output| {
        update(json!({
            "sessionUpdate": "usage_update", "used": 10, "size": 1000,
            "_meta": {"flsh/sessionUsage": {"input_tokens": input, "output_tokens": output, "tokens_unknown": false}}
        }))
    };
    live.apply(&totals(100_u64, 20_u64));
    live.apply(&totals(100, 20));
    live.spent(TurnTokens::default());
    assert_eq!(live.info.tokens, 120);
    assert_eq!(next_turn(&turns).await.tokens, 120);
    live.apply(&totals(250, 40));
    live.spent(TurnTokens::default());
    assert_eq!(live.info.tokens, 290);
    assert_eq!(next_turn(&turns).await.tokens, 170);
    live.apply(&totals(u64::MAX, 1));
    live.spent(TurnTokens::default());
    assert_eq!(live.info.tokens, 290);
}

#[test]
fn tool_details_keep_the_name_arguments_and_result_across_updates() {
    let mut live = live();
    live.prompted("check the build later");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "wait", "title": "Timer to recheck the build",
        "status": "pending", "_meta": {"claudeCode": {"toolName": "Bash"}}
    })));
    let entries = live.transcript(10).0;
    let tool = entries.last().unwrap().tool.as_ref().unwrap();
    assert_eq!(tool.name.as_deref(), Some("Bash"));
    assert!(tool.input.is_none());
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "wait", "status": "in_progress",
        "rawInput": {"command": "sleep 45", "description": "Timer to recheck the build"}
    })));
    let entries = live.transcript(10).0;
    let tool = entries.last().unwrap().tool.as_ref().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(tool.input.as_ref().unwrap()).unwrap()["command"],
        "sleep 45"
    );
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "wait", "status": "completed",
        "rawOutput": {"elapsed": 45}
    })));
    let entries = live.transcript(10).0;
    let tool = entries.last().unwrap().tool.as_ref().unwrap();
    assert_eq!(tool.name.as_deref(), Some("Bash"));
    assert_eq!(tool.result.as_deref(), Some(r#"{"elapsed":45}"#));
}

#[test]
fn a_description_does_not_invent_a_tool_name() {
    let mut live = live();
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "wait", "title": "Timer to recheck the build",
        "status": "completed", "rawInput": {"seconds": 45}
    })));
    let entries = live.transcript(10).0;
    let tool = entries.last().unwrap().tool.as_ref().unwrap();
    assert!(tool.name.is_none());
}

#[test]
fn a_replayed_compaction_summary_does_not_name_the_session() {
    let chunk = |text: &str| {
        update(json!({"sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": text}}))
    };

    for title in [None, Some("Saved name")] {
        for pieces in [
            vec![
                "This session is being continued from a previous conversation that ran out of context.\nSummary below.",
            ],
            vec![
                "This session ",
                "is being continued from a previous conversation",
                " that ran out of context.\nSummary below.",
            ],
        ] {
            let mut session = title.map_or_else(live, live_titled);

            for piece in pieces {
                session.apply(&chunk(piece));
                assert_eq!(session.info.title.as_deref(), title);
            }

            session.apply(&said("Continuing."));
            session.apply(&chunk("Fix the build"));
            assert_eq!(
                session.info.title.as_deref(),
                Some(title.unwrap_or("Fix the build"))
            );
        }
    }
}

#[test]
fn omitted_codex_spawns_are_recovered_once_without_splitting_a_reply() {
    let mut live = live();
    live.prompted("check the code");
    live.apply(&said("Checking"));
    let run = Run {
        session: Some("subagent:codex:child-1".into()),
        working: true,
        task: Some("check_code".into()),
        ..Default::default()
    };
    assert!(
        !live.recover_codex_children(std::slice::from_ref(&run)),
        "wait for the reply boundary"
    );
    live.apply(&said(" now."));
    live.info.state.status = Status::Idle;
    assert!(live.recover_codex_children(std::slice::from_ref(&run)));
    assert!(!live.recover_codex_children(std::slice::from_ref(&run)));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1].text, "Checking now.");
    let children = live.codex_children(0);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].threads, ["child-1"]);
    let (at, text) = live
        .with_run(&children[0].call, &run)
        .expect("linked child");
    assert_eq!(at, 2);
    assert!(text.starts_with("check_code [in_progress]\n[steps] 0"));
    assert!(text.contains("[session] subagent:codex:child-1"));

    let finished = Run {
        working: false,
        result: Some("Checked.".into()),
        ..run
    };
    assert!(!live.recover_codex_children(std::slice::from_ref(&finished)));
    let (end_at, text) = live
        .with_run(&children[0].call, &finished)
        .expect("finished child");
    assert_eq!(end_at, at);
    assert!(text.starts_with("check_code [completed]"));
    assert!(text.ends_with("Checked."));
}

#[test]
fn recovered_children_do_not_duplicate_native_codex_spawns() {
    let mut live = live();
    live.apply(&update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "native",
        "title": "spawnAgent", "rawInput": {"receiverThreadIds": ["child-1"]}}),
    ));
    assert!(!live.recover_codex_children(&[Run {
        session: Some("subagent:codex:child-1".into()),
        ..Default::default()
    }]));
    assert_eq!(live.codex_children(0).len(), 1);
}

#[test]
fn claude_and_opencode_spawns_accept_arguments_in_a_later_update() {
    for title in ["Agent", "Task", "task"] {
        let mut live = live();
        live.apply(&update(
            json!({"sessionUpdate": "tool_call", "toolCallId": "task",
            "title": title, "status": "in_progress"}),
        ));
        live.apply(&update(
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "task",
            "rawInput": {"subagent_type": "Explore", "prompt": "Find the loader"}}),
        ));
        let (entries, _) = live.transcript(10);
        assert!(
            entries[0].text.contains("[steps] 0\n[agent] Explore"),
            "{title}"
        );
    }
}

#[test]
fn recovery_reuses_cards_restored_after_a_restart() {
    let mut live = live();
    let run = Run {
        session: Some("subagent:codex:child-1".into()),
        task: Some("check_code".into()),
        working: true,
        ..Default::default()
    };
    live.restore(vec![run.entry()]);
    assert!(live.recover_codex_children(std::slice::from_ref(&run)));
    assert_eq!(live.transcript(10).0.len(), 1);
    let children = live.codex_children(0);
    assert_eq!(live.with_run(&children[0].call, &run).expect("card").0, 0);
}

#[test]
fn recovery_waits_for_a_native_spawn_to_name_its_thread() {
    let mut live = live();
    live.apply(&update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "native",
        "title": "spawnAgent", "status": "in_progress",
        "rawInput": {"receiverThreadIds": []}}),
    ));
    let run = Run {
        session: Some("subagent:codex:child-1".into()),
        ..Default::default()
    };
    assert!(!live.recover_codex_children(std::slice::from_ref(&run)));
    live.apply(&update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "native",
        "status": "completed", "rawInput": {"receiverThreadIds": ["child-1"]}}),
    ));
    assert!(!live.recover_codex_children(std::slice::from_ref(&run)));
    assert_eq!(live.transcript(10).0.len(), 1);
}

#[test]
fn a_background_claude_subagent_stops_counting_when_its_task_ends() {
    use crate::agents::acp::AsyncTask;

    let mut live = live();
    live.prompted("research this in the background");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "agent1", "title": "Task",
        "kind": "think", "status": "in_progress",
        "rawInput": {"subagent_type": "Explore", "description": "Research",
            "prompt": "Look around", "run_in_background": true}
    })));
    assert_eq!(live.info.state.subagents, 1);

    assert!(live.apply_task(AsyncTask::Spawned {
        async_task_id: "task1".into(),
        name: "Research".into(),
        task_type: "agent".into(),
        description: "Look around".into(),
        can_stop: true,
        output_file_path: None,
        tool_call_id: Some("agent1".into()),
    }));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "agent1", "status": "completed",
        "_meta": {"jetbrains": {"air": {"asyncTasks": {"backgrounded": true}}}}
    })));
    assert_eq!(live.info.state.subagents, 1, "its task still runs");

    assert!(live.apply_task(AsyncTask::StateUpdate {
        async_task_id: "task1".into(),
        state: "completed".into(),
        summary: None,
        output_file_path: None,
        tool_call_id: None,
    }));
    assert_eq!(live.info.state.subagents, 0, "its task ended");
}

#[test]
fn a_default_claude_subagent_is_linked_to_its_conversation() {
    let mut live = live();
    live.prompted("write the issue");
    // Claude Code leaves out `subagent_type` for its default agent.
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "agent1", "title": "Write the issue",
        "kind": "think", "status": "in_progress",
        "_meta": {"claudeCode": {"toolName": "Agent"}},
        "rawInput": {"description": "Write the issue", "prompt": "Open it", "model": "haiku"}
    })));

    assert_eq!(live.info.state.subagents, 1);
    assert_eq!(live.claude_children(0, 2), [(1, "agent1".to_owned())]);
}

#[test]
fn a_cancelled_turn_closes_the_subagent_calls_it_interrupted() {
    let mut live = live();
    live.prompted("research this");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "agent1", "title": "Task",
        "kind": "think", "status": "in_progress",
        "rawInput": {"subagent_type": "Explore", "description": "Research",
            "prompt": "Look around"}
    })));
    assert_eq!(live.info.state.subagents, 1);

    assert!(!live.orphaned_subagents);

    live.close_interrupted_subagents();
    assert_eq!(live.info.state.subagents, 0);
    assert!(
        live.orphaned_subagents,
        "Stop all still ends the agent for the subagents the stop closed"
    );
    assert!(
        live.transcript(10)
            .0
            .iter()
            .any(|entry| entry.text.contains("[failed]")),
        "its entry no longer reads as running"
    );
}

#[test]
fn a_claude_subagent_launched_in_the_background_counts_until_its_turn_ends() {
    let mut live = live();
    live.prompted("research this in the background");
    let launched = |live: &mut Live, id: &str| {
        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": id, "title": "Task",
            "kind": "think", "status": "pending",
            "rawInput": {"subagent_type": "Explore", "prompt": "Look around",
                "run_in_background": true}
        })));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update", "toolCallId": id,
            "_meta": {"claudeCode": {"toolResponse": {"status": "async_launched", "isAsync": true}}}
        })));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed"
        })));
    };
    launched(&mut live, "agent1");
    assert_eq!(
        live.info.state.subagents, 1,
        "its call returned, it runs on"
    );

    // A stop spares it.
    live.close_interrupted_subagents();
    assert_eq!(live.info.state.subagents, 1);

    // Claude Code's own turn after its report ends it.
    use crate::agents::acp::AsyncTask;
    assert!(!live.apply_task(AsyncTask::Spawned {
        async_task_id: "turn1".into(),
        name: "Claude is working".into(),
        task_type: "turn".into(),
        description: String::new(),
        can_stop: true,
        output_file_path: None,
        tool_call_id: None,
    }));
    assert_eq!(live.info.state.subagents, 1);
    assert!(live.apply_task(AsyncTask::StateUpdate {
        async_task_id: "turn1".into(),
        state: "completed".into(),
        summary: None,
        output_file_path: None,
        tool_call_id: None,
    }));
    assert_eq!(live.info.state.subagents, 0);

    // A turn that ends by itself ends them too: Claude Code held it for them.
    live.prompted("again");
    launched(&mut live, "agent2");
    assert_eq!(live.info.state.subagents, 1);
    live.end_background_subagents();
    assert_eq!(live.info.state.subagents, 0);
}

#[test]
fn a_turn_claude_code_starts_on_its_own_shows_as_working() {
    use crate::agents::acp::AsyncTask;
    let spawned = |id: &str| AsyncTask::Spawned {
        async_task_id: id.into(),
        name: "Claude is working".into(),
        task_type: "turn".into(),
        description: String::new(),
        can_stop: true,
        output_file_path: None,
        tool_call_id: None,
    };
    let ended = |id: &str| AsyncTask::StateUpdate {
        async_task_id: id.into(),
        state: "completed".into(),
        summary: None,
        output_file_path: None,
        tool_call_id: None,
    };
    let mut live = live();
    live.info.state.status = Status::Idle;

    assert!(live.apply_task(spawned("turn1")));
    assert_eq!(
        live.status(),
        Status::Working,
        "a background task's report woke it"
    );
    assert!(live.apply_task(ended("turn1")));
    assert_eq!(live.status(), Status::Idle);

    // A prompt folded into that turn ends before it: the turn goes on.
    live.apply_task(spawned("turn2"));
    live.prompted("so?");
    live.info.state.status = Status::Idle;
    assert!(live.show_autonomous_turn());
    assert_eq!(live.status(), Status::Working);
    live.apply_task(ended("turn2"));
    assert_eq!(live.status(), Status::Idle);

    // A prompt that runs past that turn's end keeps its own status.
    live.apply_task(spawned("turn3"));
    live.prompted("go on");
    live.apply_task(ended("turn3"));
    assert_eq!(live.status(), Status::Working);
}

#[test]
fn a_live_claude_spawn_shows_its_requested_model() {
    let mut live = live();
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "spawn", "title": "Agent",
        "kind": "other", "status": "in_progress",
        "rawInput": {"subagent_type": "general-purpose", "model": "sonnet"}
    })));
    let (entries, _) = live.transcript(10);
    assert!(entries.iter().any(|entry| {
        entry
            .text
            .contains("[agent] general-purpose\n[model] sonnet")
    }));
}

#[test]
fn automatic_titles_are_cleaned_on_updates_and_restore() {
    let tagged =
        crate::tags::from_session("parent", "Parent task", "Fix the upload queue.\nDetails");
    let mut session = live_titled(&tagged);
    assert_eq!(session.info.title.as_deref(), Some("Fix the upload queue."));
    session.apply(&update(
        json!({"sessionUpdate": "session_info_update", "title": tagged}),
    ));
    assert_eq!(session.info.title.as_deref(), Some("Fix the upload queue."));
    session.apply(&update(
        json!({"sessionUpdate": "session_info_update", "title": "<from-session id=\"parent\""}),
    ));
    assert_eq!(session.info.title.as_deref(), Some("Fix the upload queue."));
    session.apply(&update(
        json!({"sessionUpdate": "session_info_update", "title": "task ".repeat(40)}),
    ));
    assert!(session.info.title.as_ref().unwrap().chars().count() <= 80);
}

#[test]
fn streamed_prompt_titles_wait_for_complete_headers() {
    let mut session = live();

    for chunk in [
        "<from-",
        "session id=\"parent\" name=\"Parent",
        " task\">\n",
        "Fix the upload queue.",
        "\n</from-session>",
    ] {
        session.apply(&update(json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": chunk}})));
        if chunk.contains("queue") || chunk.contains("</from-session>") {
            assert_eq!(session.info.title.as_deref(), Some("Fix the upload queue."));
        } else {
            assert_eq!(session.info.title, None);
        }
    }
}

#[test]
fn recorded_times_survive_streaming_and_replay() {
    let mut live = live();
    live.prompted("fix it");
    live.apply_at(&said("Fixed"), Instant::now());
    let before = live.transcript(10).0;
    assert!(before.iter().all(|entry| entry.sent_at.is_some()));
    live.apply_at(&said("."), Instant::now());
    assert_eq!(live.transcript(10).0[1].sent_at, before[1].sent_at);

    let saved = live.transcript(10).0;
    let mut replay = live_titled("fix it");
    replay.info.state.status = Status::Starting;
    replay.apply_at(&update(json!({"sessionUpdate":"user_message_chunk", "content":{"type":"text","text":"fix it"}})), Instant::now());
    replay.apply_at(&said("Fixed."), Instant::now());
    assert!(
        replay
            .transcript(10)
            .0
            .iter()
            .all(|entry| entry.sent_at.is_none())
    );
    replay.restore_times(&saved);
    assert_eq!(replay.transcript(10).0, saved);
}

#[test]
fn recovery_context_does_not_name_a_conversation() {
    let mut live = live();
    live.prompted(&crate::tags::context(crate::agents::session::CONTINUE));
    assert!(live.info.title.is_none());
    assert!(crate::tags::own_words(&live.transcript(10).0[0].text).is_empty());
}

#[test]
fn replay_dates_match_the_saved_tail_when_prompts_repeat() {
    let mut replay = live();
    replay.info.state.status = Status::Starting;
    replay.push(EntryKind::User, "yes");
    replay.push(EntryKind::Agent, "Done.");
    replay.push(EntryKind::User, "yes");
    replay.push(EntryKind::Agent, "Done.");
    let mut saved = beep_proto::node::agent::Entry::new(EntryKind::User, "yes".into());
    saved.sent_at = Some(123);
    replay.restore_times(&[saved]);
    let entries = replay.transcript(10).0;
    assert_eq!(entries[0].sent_at, None);
    assert_eq!(entries[2].sent_at, Some(123));
}

#[test]
fn a_running_file_send_shows_the_bytes_sent_until_it_ends() {
    let mut live = live();
    live.prompted("send the build");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "send", "title": "mcp__beep__files",
        "status": "in_progress", "_meta": {"claudeCode": {"toolName": "mcp__beep__files"}},
        "rawInput": {"action": "send", "machine": "build-box", "paths": ["app.apk"]}
    })));
    let sent = state::Sent {
        done: 400,
        total: 1200,
    };

    assert!(live.sending(sent));
    assert!(!live.sending(sent));
    let text = live.transcript(10).0.last().unwrap().text.clone();
    assert_eq!(
        text,
        "Beep: Send 1 file to build-box [in_progress]\n[sent] 400 1200"
    );

    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "send", "status": "completed"
    })));
    let text = live.transcript(10).0.last().unwrap().text.clone();
    assert_eq!(text, "Beep: Send 1 file to build-box [completed]");
}

#[test]
fn a_plan_step_keeps_when_it_ran_across_plan_updates() {
    let plan = |steps: &[(&str, &str)]| {
        let entries: Vec<Value> = steps
            .iter()
            .map(|(content, status)| json!({"content": content, "priority": "medium", "status": status}))
            .collect();
        update(json!({"sessionUpdate": "plan", "entries": entries}))
    };
    let mut live = live();

    // Loaded history is not dated: nobody knows when it ran.
    live.apply(&plan(&[("Build", "in_progress")]));
    assert_eq!(live.info.state.plan[0].started_at, None);

    live.info.state.status = Status::Idle;
    live.info.state.plan[0].started_at = Some(5);
    live.apply(&plan(&[
        ("Build", "completed"),
        ("Review", "in_progress"),
        ("Ship", "pending"),
        ("Skipped", "completed"),
    ]));
    let [build, review, ship, skipped] = &live.info.state.plan[..] else {
        panic!("four steps");
    };
    assert_eq!(build.started_at, Some(5));
    assert!(build.ended_at.is_some_and(|at| at > 5));
    assert!(review.started_at.is_some() && review.ended_at.is_none());
    assert_eq!((ship.started_at, ship.ended_at), (None, None));
    assert!(skipped.started_at.is_some() && skipped.started_at == skipped.ended_at);

    // A step taken up again runs once more.
    live.apply(&plan(&[("Build", "in_progress")]));
    assert_eq!(live.info.state.plan[0].started_at, Some(5));
    assert_eq!(live.info.state.plan[0].ended_at, None);
}

#[test]
fn image_only_prompts_wait_for_text_before_naming_the_session() {
    let marker = "[image: image-0123456789abcdef]";
    let mut started = live();
    started.prompted(marker);
    assert_eq!(started.info.title, None);
    assert_eq!(started.transcript.back().unwrap().text, marker);
    started.prompted("Fix the overlap");
    assert_eq!(started.info.title.as_deref(), Some("Fix the overlap"));

    let mut replayed = live();
    for text in ["[image: image-", "0123456789abcdef]\n"] {
        replayed.apply(&update(json!({
            "sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": text}
        })));
        assert_eq!(replayed.info.title, None);
    }
    replayed.apply(&update(json!({
        "sessionUpdate": "user_message_chunk",
        "content": {"type": "text", "text": "Fix the overlap"}
    })));
    assert_eq!(replayed.info.title.as_deref(), Some("Fix the overlap"));
}

#[test]
fn flsh_agents_show_progress_and_stop_counting_when_finished() {
    let mut live = live();
    live.prompted("Check files");
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "flsh-agent:worker",
        "name": "flsh_agent", "title": "Check files", "status": "in_progress",
        "rawInput": {"flshAgent": true, "name": "worker", "model": "provider:model"}
    })));
    assert_eq!(live.info.state.subagents, 1);
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "flsh-agent:worker:read",
        "title": "read: src/lib.rs", "status": "in_progress",
        "_meta": {"flsh": {"parentToolUseId": "flsh-agent:worker"}}
    })));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "flsh-agent:worker:read",
        "status": "completed"
    })));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 2);
    assert!(
        entries[1]
            .text
            .contains("[step completed] read: src/lib.rs")
    );
    assert!(entries[1].text.contains("[agent] worker"));
    assert!(entries[1].text.contains("[model] provider:model"));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "flsh-agent:worker",
        "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "Checked."}}]
    })));
    assert_eq!(live.info.state.subagents, 0);
    let (entries, _) = live.transcript(10);
    assert!(entries[1].text.starts_with("Check files [completed]"));
    assert!(entries[1].text.ends_with("Checked."));
}

#[test]
fn replayed_flsh_delegations_keep_the_agent_card() {
    let mut live = live();
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "delegate-1",
        "title": "delegate", "status": "in_progress",
        "rawInput": {"task": "Check files", "name": "worker", "model": "provider:model"}
    })));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "delegate-1",
        "status": "completed"
    })));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].text.starts_with("Check files [completed]"));
    assert!(entries[0].text.contains("[agent] worker"));
}

#[test]
fn flsh_beep_mcp_calls_with_summarized_arguments_format_as_beep_calls() {
    let mut live = live();
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "start-1",
        "title": "mcp__beep__start_agent: background, cwd, harness, machine, prompt, title",
        "status": "in_progress"
    })));
    let (entries, _) = live.transcript(10);
    assert_eq!(entries[0].text, "Beep: Start an agent [in_progress]");

    live.apply(&update(json!({
        "sessionUpdate": "tool_call_update", "toolCallId": "start-1",
        "status": "completed",
        "content": [{"type": "content", "content": {"type": "text", "text": "{\"id\":\"a1@build-box\"}"}}]
    })));
    let (entries, _) = live.transcript(10);
    assert_eq!(
        entries[0].text,
        "Beep: Start an agent [completed]\n{\"id\":\"a1@build-box\"}"
    );
}

#[test]
fn antigravity_window_keeps_native_invocation_ids_beside_beep_children() {
    let mut live = live();
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "call-native",
        "name": "invoke_subagent", "title": "Run invoke_subagent",
        "status": "completed", "kind": "execute"
    })));
    live.apply(&update(json!({
        "sessionUpdate": "tool_call", "toolCallId": "call-linked",
        "title": "mcp__beep__start_agent: title", "status": "completed"
    })));
    assert_eq!(live.agy_calls(0, 2), [(0, "call-native".into())]);
    assert!(live.agy_calls(1, 1).is_empty());
    assert!(live.transcript(2).0[1].text.starts_with("Beep: Start"));
}

/// The next finished turn, taken as a stream does: read, then sent.
async fn next_turn(
    turns: &crate::agents::alerts::Follow<'_, beep_proto::node::usage::TurnUsage>,
) -> beep_proto::node::usage::TurnUsage {
    let turn = turns.next().await;
    turns.sent();
    turn
}
