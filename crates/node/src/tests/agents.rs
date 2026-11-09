use super::kinds::launch;
use super::running::home_twins;
use super::*;
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{self, CheckoutPlan, SessionsReply, Status},
    harness::AgentSettings,
};
use std::{collections::BTreeMap, path::Path};

/// A Claude Code session id, as its transcript file is named.
#[cfg(unix)]
const CLAUDE_ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";

/// An adapter that, loading a session, replays a prompt with `$1`
/// inlined as a data URL, as Codex does, then waits.
#[cfg(unix)]
const REPLAYING_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
read -r line
printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"what is this [@shot.png](data:image/png;base64,%s)"}}}}\n' "$1"
reply '{}'
while read -r line; do reply '{}'; done
"#;

/// An agent that runs `script` in sh.
#[cfg(unix)]
fn sh_agent(name: &str, script: &str) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
    }
}

#[cfg(unix)]
fn png(fill: u8) -> Vec<u8> {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.resize(48, fill);
    data
}

/// Agents that read Claude Code's sessions in `root`.
#[cfg(unix)]
fn agents_at(root: &std::path::Path) -> Agents {
    Agents {
        external: External::at(root),
        ..Agents::default()
    }
}

/// An adapter whose one turn thinks in chunks, as Codex's reasoning
/// summaries arrive, then answers.
#[cfg(unix)]
const THINKING_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
update() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"%s","content":{"type":"text","text":"%s"}}}}\n' "$1" "$2"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply '{"sessionId":"s"}'
while read -r line; do
case "$line" in
*session/prompt*)
  update agent_thought_chunk '\n\n'
  update agent_thought_chunk '**Checking 391**'
  update agent_thought_chunk '\n\nIt is 17 x'
  update agent_thought_chunk ' 23.'
  update agent_message_chunk 'Not prime.'
  reply '{"stopReason":"end_turn"}' ;;
*) reply '{}' ;;
esac
done
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_transcript_read_gives_thoughts_only_to_a_server_that_asks() {
    let root = tempfile::tempdir().expect("a temp folder");
    let projects = root.path().join("claude/projects/-srv-project");
    std::fs::create_dir_all(&projects).expect("a projects folder");
    // A session started outside Beep, read from Claude Code's file.
    let lines = [
        serde_json::json!({"type": "user", "timestamp": "2026-10-29T08:00:00.000Z",
            "message": {"role": "user", "content": "is 391 prime?"}}),
        serde_json::json!({"type": "assistant", "timestamp": "2026-10-29T08:00:04.000Z",
            "message": {"content": [{"type": "thinking", "thinking": "Try 17.", "signature": "s"}]}}),
        serde_json::json!({"type": "assistant", "timestamp": "2026-10-29T08:00:05.000Z",
            "message": {"content": [{"type": "text", "text": "No."}]}}),
    ];
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(projects.join(format!("{CLAUDE_ID}.jsonl")), text).expect("a transcript");
    let agents = agents_at(&root.path().join("claude"));
    let kinds = |reply: SessionsReply| -> Vec<(agent::EntryKind, String, Option<u64>)> {
        match reply {
            SessionsReply::Transcript { entries, .. } => entries
                .into_iter()
                .map(|entry| (entry.kind, entry.text, entry.ms))
                .collect(),
            other => panic!("not a transcript: {other:?}"),
        }
    };
    assert_eq!(
        kinds(agents.transcript_reply(CLAUDE_ID, 10, true, None).await),
        [
            (agent::EntryKind::User, "is 391 prime?".into(), None),
            (agent::EntryKind::Thought, "Try 17.".into(), Some(4000)),
            (agent::EntryKind::Agent, "No.".into(), None),
        ]
    );
    assert_eq!(
        kinds(agents.transcript_reply(CLAUDE_ID, 10, false, None).await).len(),
        2
    );

    // A session this node runs, read from what it kept of the turn.
    let kind = sh_agent("codex", THINKING_ADAPTER);
    let begin = Begin {
        prompt: Some("is 391 prime?".into()),
        ..Begin::default()
    };
    let session = agents
        .start(
            &kind,
            Some(root.path().to_str().expect("a UTF-8 path")),
            begin,
        )
        .expect("the session starts");
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|live| {
            live.status() == Status::Idle
                && live
                    .transcript(10)
                    .0
                    .iter()
                    .any(|entry| entry.text == "Not prime.")
        }),
    )
    .await
    .expect("the turn ends")
    .expect("the session is still there");
    let read = kinds(agents.transcript_reply(&session.id, 10, true, None).await);
    let shown: Vec<(agent::EntryKind, &str)> = read
        .iter()
        .map(|(kind, text, _)| (*kind, text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (agent::EntryKind::User, "is 391 prime?"),
            (
                agent::EntryKind::Thought,
                "**Checking 391**\n\nIt is 17 x 23."
            ),
            (agent::EntryKind::Agent, "Not prime."),
        ]
    );
    assert!(read[1].2.is_some(), "a live thought says how long it took");
    let without = kinds(agents.transcript_reply(&session.id, 10, false, None).await);
    assert!(
        without
            .iter()
            .all(|(kind, _, _)| *kind != agent::EntryKind::Thought)
    );
    session.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_replayed_image_is_served_and_found_again_once_evicted() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use beep_proto::image_key;
    let dir = std::env::temp_dir().join(format!("beep-agent-images-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let projects = dir.join("claude/projects/-srv-project");
    std::fs::create_dir_all(&projects).unwrap();
    let shot = png(1);
    let encoded = STANDARD.encode(&shot);
    // Claude Code's own file of the session holds the image too.
    let line = serde_json::json!({"type": "user", "message": {"role": "user", "content": [
        {"type": "text", "text": "what is this"},
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": encoded}}
    ]}});
    std::fs::write(
        projects.join(format!("{CLAUDE_ID}.jsonl")),
        format!("{line}\n"),
    )
    .unwrap();
    let agents = agents_at(&dir.join("claude"));
    let kind = AgentKind {
        name: "claude".into(),
        program: "sh".into(),
        args: vec![
            "-c".into(),
            REPLAYING_ADAPTER.into(),
            "fake-adapter".into(),
            encoded,
        ],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
    };
    let begin = Begin {
        resume: Some(CLAUDE_ID.into()),
        ..Begin::default()
    };
    let session = agents
        .start(&kind, Some(dir.to_str().unwrap()), begin)
        .unwrap();
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|live| matches!(live.status(), Status::Idle | Status::Ended)),
    )
    .await
    .expect("the session loads")
    .unwrap();
    let key = image_key(&shot);
    let (entries, _) = session.live.borrow().transcript(10);
    assert_eq!(
        entries[0].text,
        format!("what is this\n[image: image-{key}]\n")
    );
    let served = agents.image(&session.id, &key).await.unwrap();
    assert_eq!(
        served.map(|picture| picture.data.clone()),
        Some(shot.clone())
    );
    // Newer pictures push it out; the session's file still has it.
    for fill in 2..40 {
        agents.images.keep(
            &session.id,
            Picture {
                mime: "image/png".into(),
                data: png(fill),
            },
        );
    }
    assert!(agents.images.get(&session.id, &key).is_none(), "evicted");
    let found = agents.image(&session.id, &key).await.unwrap();
    assert_eq!(
        found.map(|picture| picture.data.clone()),
        Some(shot.clone())
    );
    assert!(agents.images.get(&session.id, &key).is_some(), "kept again");
    // A session started outside Beep is read from its file by its id.
    let outside = agents.image(CLAUDE_ID, &key).await.unwrap();
    assert_eq!(outside.map(|picture| picture.data.clone()), Some(shot));
    let missing = agents.image(CLAUDE_ID, &image_key(b"other")).await;
    assert!(missing.unwrap().is_none());
    session.close().await;
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn every_known_agent_starts_without_permission_prompts() {
    let launched = |name: &str| {
        let known = KNOWN
            .iter()
            .find(|known| known.name == name)
            .expect("a known agent");
        let Some(kinds::Launch::Ready(kind)) = launch(known, &|_: &str| true, None) else {
            panic!("{name} is installed");
        };
        let env: Vec<(String, String)> = kind.env.into_iter().collect();
        (kind.args, env)
    };
    let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    // Claude Code is switched to bypassPermissions by the session, and
    // shows its thinking as summaries, unless the node's own environment
    // already chose how.
    let thinking = std::env::var_os("CLAUDE_CODE_THINKING_DISPLAY")
        .is_none()
        .then(|| pair("CLAUDE_CODE_THINKING_DISPLAY", "summarized"));
    assert_eq!(launched("claude"), (vec![], thinking.into_iter().collect()));
    assert_eq!(
        launched("codex"),
        (
            vec![],
            vec![pair("INITIAL_AGENT_MODE", "agent-full-access")]
        )
    );
    assert_eq!(
        launched("gemini"),
        (
            vec!["--acp".into(), "--approval-mode".into(), "yolo".into()],
            vec![pair("GEMINI_CLI_TRUST_WORKSPACE", "true")]
        )
    );
    assert_eq!(
        launched("opencode"),
        (
            vec!["acp".into()],
            vec![pair("OPENCODE_PERMISSION", r#"{"*":"allow"}"#)]
        )
    );
    // Neither has permission prompts to turn off.
    assert_eq!(launched("pi"), (vec![], vec![]));
    assert_eq!(launched("flsh"), (vec!["acp".into()], vec![]));
    assert_eq!(launched("hermes"), (vec!["acp".into()], vec![]));
    assert_eq!(
        kinds::known("hermes").unwrap().bypass_mode,
        Some("dont_ask")
    );
    let rules: serde_json::Value =
        serde_json::from_str(&launched("opencode").1[0].1).expect("JSON");
    assert_eq!(rules, serde_json::json!({"*": "allow"}));
}

#[test]
fn a_folder_must_exist_and_be_absolute() {
    assert!(folder(Some(if cfg!(windows) { "C:\\" } else { "/" })).is_ok());
    assert!(folder(None).is_ok());
    assert!(folder(Some("relative/dir")).is_err());
    assert!(folder(Some("/no/such/folder/here")).is_err());
}

#[test]
fn another_machines_home_folder_maps_here() {
    let home = Path::new("/home/you");
    assert_eq!(
        home_twins(Path::new("/Users/you/Projects/work/tool"), home),
        [
            PathBuf::from("/home/you/Projects/work/tool"),
            PathBuf::from("/home/you/work/tool"),
        ]
    );
    assert_eq!(
        home_twins(Path::new("/home/you/work/tool"), home),
        [PathBuf::from("/home/you/Projects/work/tool")]
    );
    assert!(home_twins(Path::new("/srv/data"), home).is_empty());
    assert!(
        home_twins(Path::new("/home/you"), home).is_empty()
            || home_twins(Path::new("/home/you"), home) == [PathBuf::from("/home/you/Projects")]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn one_session_cannot_be_taken_over_twice() {
    // An adapter that never answers: the session stays starting.
    let kind = sh_agent("claude", "cat > /dev/null");
    let agents = Agents::new(vec![kind.clone()]);
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let begin = || Begin {
        resume: Some(id.to_owned()),
        ..Begin::default()
    };
    let first = agents.start(&kind, Some("/"), begin()).unwrap();
    let second = agents.start(&kind, Some("/"), begin()).err().unwrap();
    assert!(
        second
            .to_string()
            .contains(&format!("already runs here as {}", first.id)),
        "{second}"
    );
    assert_eq!(agents.owned().get(id), Some(&first.id));
}

#[cfg(unix)]
#[tokio::test]
async fn an_ended_node_session_resumes_from_the_agents_session() {
    let kind = sh_agent("claude", "cat > /dev/null");
    let agents = Agents::new(vec![kind.clone()]);
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let begin = Begin {
        resume: Some(id.to_owned()),
        ..Begin::default()
    };
    let first = agents.start(&kind, Some("/"), begin).unwrap();
    first.close().await;
    // Nothing on this machine has the agent's session, so it is the
    // agent's id that the node looks for, not its own.
    let err = agents.resume(&kind, &first.id, None).await.err().unwrap();
    assert!(err.to_string().contains(id), "{err}");
}

#[cfg(unix)]
#[tokio::test]
async fn hermes_sessions_started_here_resume_over_acp_without_a_store_reader() {
    let kind = sh_agent(
        "hermes",
        r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
loaded=false
while read -r line; do
case "$line" in
*'"initialize"'*) reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}' ;;
*'"session/new"'*) reply '{"sessionId":"hermes-session"}' ;;
*'"session/load"'*) loaded=true; reply '{}' ;;
*'"session/prompt"'*)
  if [ "$loaded" != true ]; then reply '{"stopReason":"end_turn"}'; continue; fi
  echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"hermes-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Resumed."}}}}'
  reply '{"stopReason":"end_turn"}' ;;
*) reply '{}' ;;
esac
done
"#,
    );
    let agents = Agents::new(vec![kind.clone()]);
    let root = tempfile::tempdir().unwrap();
    let first = agents
        .start(
            &kind,
            root.path().to_str(),
            Begin {
                mode: Some("accept_edits".into()),
                ..Begin::default()
            },
        )
        .unwrap();
    let mut live = first.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|live| live.status() == Status::Idle || live.status() == Status::Ended),
    )
    .await
    .unwrap()
    .unwrap();
    first.close().await;
    let resumed = agents
        .resume(&kind, "hermes-session", Some("continue".into()))
        .await
        .unwrap();
    let mut live = resumed.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|live| live.info.turns == 1 || live.status() == Status::Ended),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(resumed.info().turns, 1, "{:?}", resumed.info().state.error);
    assert_eq!(resumed.info().cwd, first.info().cwd);
    assert_eq!(resumed.info().mode.as_deref(), Some("accept_edits"));
    assert!(
        resumed
            .live
            .borrow()
            .transcript(10)
            .0
            .iter()
            .any(|entry| entry.text == "Resumed.")
    );
    resumed.close().await;
    assert!(agents.resume(&kind, "outside-session", None).await.is_err());
}

/// Answers `initialize` and `session/new` with the agent's own id, then
/// anything else with nothing, until its input closes.
#[cfg(unix)]
const NAMING_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply '{"sessionId":"0199b2c3-4d5e-7f60-8a9b-0c1d2e3f4a5b"}'
while read -r line; do reply '{}'; done
"#;

/// A session goes by the agent's id once it gives one, and the node
/// stopping ends it as such. After a restart that id is resumed from
/// the agent's own logs.
#[cfg(unix)]
#[tokio::test]
async fn a_session_goes_by_the_agents_id_and_ends_with_the_node() {
    const AGENT_ID: &str = "0199b2c3-4d5e-7f60-8a9b-0c1d2e3f4a5b";
    let kind = sh_agent("codex", NAMING_ADAPTER);
    let agents = Agents::new(vec![kind.clone()]);
    let begin = Begin::default();
    let session = agents.start(&kind, Some("/"), begin).unwrap();
    assert!(session.info().id.is_empty());
    assert!(
        agents.list().is_empty(),
        "ACP has not named this session yet"
    );
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|live| live.agent_session.is_some()),
    )
    .await
    .expect("the agent gives its id")
    .expect("the session is still there");
    assert_eq!(session.info().id, AGENT_ID);
    assert_eq!(agents.list()[0].id, AGENT_ID);
    assert!(Arc::ptr_eq(&agents.get(AGENT_ID).unwrap(), &session));
    agents.shutdown().await;
    let ended = session.info();
    assert_eq!(ended.state.status, Status::Ended);
    assert_eq!(ended.state.error.as_deref(), Some(session::NODE_STOPPED));
}

#[cfg(unix)]
#[tokio::test]
async fn a_resumed_session_replaces_the_ended_run_in_listings() {
    const AGENT_ID: &str = "0199b2c3-4d5e-7f60-8a9b-0c1d2e3f4a5b";
    let script = NAMING_ADAPTER
        .replace(
            "\"agentCapabilities\":{}",
            "\"agentCapabilities\":{\"loadSession\":true}",
        )
        .replace(
            "while read -r line; do reply '{}'; done",
            r#"while read -r line; do
case "$line" in
*session/prompt*) reply '{"stopReason":"end_turn"}' ;;
*) reply '{}' ;;
esac
done"#,
        );
    let kind = sh_agent("codex", &script);
    let agents = Agents::new(vec![kind.clone()]);
    let first = agents.start(&kind, Some("/"), Begin::default()).unwrap();
    first.named_info().await.unwrap();
    assert_ne!(first.id, AGENT_ID);
    first.close().await;

    let resumed = agents
        .start(
            &kind,
            Some("/"),
            Begin {
                resume: Some(AGENT_ID.into()),
                ..Begin::default()
            },
        )
        .unwrap();
    let mut live = resumed.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|live| live.status() == Status::Idle),
    )
    .await
    .expect("the resumed session loads")
    .unwrap();
    let listed = agents.list();
    assert_eq!(
        listed.len(),
        1,
        "the ended run must not shadow the resumed one"
    );
    assert_eq!(listed[0].id, AGENT_ID);
    assert_eq!(listed[0].state.status, Status::Idle);
    assert!(agents.get(&first.id).is_none());
    let current = agents.get(AGENT_ID).unwrap();
    assert!(Arc::ptr_eq(&current, &resumed));
    current
        .order(Command::Prompt("Continue the work".to_owned().into()))
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        live.wait_for(|live| live.info.turns == 1 && live.status() == Status::Idle),
    )
    .await
    .expect("the prompt reaches the resumed run")
    .unwrap();
    assert!(resumed.live.borrow().transcript(10).0.iter().any(|entry| {
        entry.kind == agent::EntryKind::User && entry.text == "Continue the work"
    }));
    resumed.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_refused_start_makes_no_branch() {
    let (_dir, work) = checkout::tests::repo("refused");
    let kind = sh_agent("codex", "cat > /dev/null");
    let agents = Agents::new(vec![kind.clone()]);
    agents.set_settings(AgentSettings {
        harnesses: vec![beep_proto::node::harness::HarnessConfig {
            enabled: false,
            ..beep_proto::node::harness::HarnessConfig::new("codex")
        }],
        ..AgentSettings::default()
    });
    let begin = Begin {
        prompt: Some("Fix the login bug".into()),
        ..Begin::default()
    };
    let plan = CheckoutPlan::NewBranch { base: None };
    let refused = agents
        .start_in(&kind, work.to_str(), Some(plan), begin)
        .await
        .err();
    assert!(refused.is_some_and(|e| e.to_string().contains("turned off")));
    assert_eq!(checkout::tests::current(&work), "main");
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_resumes_only_on_its_own_agent() {
    let kind = |name: &str| sh_agent(name, "cat > /dev/null");
    let (claude, codex) = (kind("claude"), kind("codex"));
    let agents = Agents::new(vec![claude.clone(), codex.clone()]);
    let begin = Begin {
        resume: Some("0199a1b2-3c4d-7e5f-8a9b-0c1d2e3f4a5b".to_owned()),
        ..Begin::default()
    };
    let running = agents.start(&codex, Some("/"), begin).unwrap();
    let err = agents.resume(&claude, &running.id, None).await.err();
    assert!(
        err.is_some_and(|e| e.to_string().contains("runs codex")),
        "a claude stream took over a codex session"
    );
    running.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn sessions_list_with_no_agent_program_found() {
    let root = tempfile::tempdir().expect("a temp folder");
    let agents = Agents {
        external: External::locate_in(root.path()),
        ..Agents::default()
    };
    assert_eq!(
        agents.offers(),
        vec![
            agent::SESSIONS_PROTOCOL.to_owned(),
            agent::SESSION_FOLLOW_PROTOCOL.to_owned(),
            beep_proto::node::events::PROTOCOL.to_owned()
        ]
    );
    let project = root.path().join("projects/-srv-project");
    std::fs::create_dir_all(&project).expect("folders");
    let line = r#"{"type":"user","cwd":"/srv/project","timestamp":"2026-11-02T10:00:00.000Z","message":{"role":"user","content":"hi"}}"#;
    let file = project.join(format!("{CLAUDE_ID}.jsonl"));
    std::fs::write(file, format!("{line}\n")).expect("a transcript");
    let listed = agents.external.list(10, None, HashMap::new()).await;
    assert_eq!(listed.expect("a list").len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn a_live_view_survives_idle_time_and_detaching_keeps_the_agent_running() {
    use agent::{FollowSession, SessionUpdate};
    use tokio::time::{advance, pause, resume, timeout};
    let root = tempfile::tempdir().unwrap();
    let agents = Arc::new(Agents {
        stores: found::Stores::default(),
        ..Agents::default()
    });
    let kind = sh_agent("codex", THINKING_ADAPTER);
    let session = agents
        .start(&kind, root.path().to_str(), Begin::default())
        .unwrap();
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(10),
        live.wait_for(|live| live.status() == Status::Idle),
    )
    .await
    .unwrap()
    .unwrap();
    let request = FollowSession {
        agent: "codex".into(),
        session: Into::<SessionId>::into(session.info().id),
        last: 200,
        after: None,
    };
    let link = beep_proto::node::local_link();
    let task = tokio::spawn(serve_follow(link.node.0, link.node.1, agents.clone()));
    let (mut tx, mut rx) = link.caller;
    tx.send(&request).await.unwrap();
    let initial = rx.recv::<SessionUpdate>().await.unwrap().unwrap();
    assert!(matches!(initial, SessionUpdate::Snapshot(_)));
    pause();
    advance(Duration::from_secs(10 * 60)).await;
    assert_eq!(
        rx.recv::<SessionUpdate>().await.unwrap(),
        Some(SessionUpdate::KeepAlive)
    );
    resume();
    session
        .order(Command::Prompt("is 391 prime?".to_owned().into()))
        .await
        .unwrap();
    timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv::<SessionUpdate>().await.unwrap().unwrap() {
                SessionUpdate::Snapshot(snapshot) | SessionUpdate::Delta(snapshot)
                    if snapshot
                        .entries
                        .iter()
                        .any(|entry| entry.text == "Not prime.") =>
                {
                    break;
                }
                SessionUpdate::Error { reason } => panic!("{reason}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    drop(tx);
    timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_ne!(session.info().state.status, Status::Ended);
    session
        .order(Command::Prompt("again".to_owned().into()))
        .await
        .unwrap();
    timeout(
        Duration::from_secs(10),
        live.wait_for(|live| live.info.turns >= 2),
    )
    .await
    .unwrap()
    .unwrap();
    session.close().await;
}

#[cfg(all(test, unix))]
mod actor_tests {
    use super::*;
    use crate::link::{Offers, serve_events as serve};
    use beep_proto::node::events::{CallReply, Operation, Reply, Request, Topic};
    use beep_proto::node::{agent, local_link};
    use std::time::Duration;
    use tokio::time::timeout;
    use tokio_util::task::AbortOnDropHandle;

    #[tokio::test]
    async fn a_prompt_and_actor_changes_travel_both_ways_without_owning_the_session() {
        let root = tempfile::tempdir().unwrap();
        let kind = AgentKind {
            name: "codex".into(), program: "sh".into(),
            args: vec!["-c".into(), r#"
                id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
                reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
                read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
                read -r line; reply '{"sessionId":"s"}'
                while read -r line; do
                    case "$line" in
                        *session/prompt*)
                            printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"received once"}}}}'
                            reply '{"stopReason":"end_turn"}' ;;
                        *) reply '{}' ;;
                    esac
                done
            "#.into()],
            env: BTreeMap::new(), compact: None, compact_models: Vec::new(),
            default_model: None,
        };
        let agents = Arc::new(Agents::new(vec![kind.clone()]));
        let session = agents
            .start(&kind, root.path().to_str(), Begin::default())
            .unwrap();
        let public_id = session.named_info().await.unwrap().id;
        let mut live = session.live.clone();
        timeout(
            Duration::from_secs(5),
            live.wait_for(|live| live.status() == agent::Status::Idle),
        )
        .await
        .unwrap()
        .unwrap();
        let offers = Offers {
            agents: agents.clone(),
            ..Offers::default()
        };
        let link = local_link();
        let task = AbortOnDropHandle::new(tokio::spawn(serve(
            link.node.0,
            link.node.1,
            offers,
            iroh::SecretKey::generate().public(),
        )));
        let (mut tx, mut rx) = link.caller;
        tx.send(&Request::Watch {
            id: 1,
            topics: vec![Topic::keyed("agent", &public_id)],
            queries: vec![],
        })
        .await
        .unwrap();
        loop {
            if matches!(rx.recv::<Reply>().await.unwrap(), Some(Reply::Snapshot(_))) {
                break;
            }
        }
        tx.send(&Request::Call {
            id: 2,
            operation: Box::new(Operation::Agent {
                agent: "codex".into(),
                session: public_id.clone(),
                request: agent::Request::Prompt {
                    text: "hello".into(),
                },
            }),
        })
        .await
        .unwrap();
        let mut acknowledged = false;
        let mut changed = false;
        timeout(Duration::from_secs(5), async {
            while !acknowledged || !changed {
                match rx.recv::<Reply>().await.unwrap().unwrap() {
                    Reply::Result { id: 2, reply } => {
                        assert!(matches!(*reply, CallReply::Agent { .. }));
                        acknowledged = true;
                    }
                    Reply::Snapshot(event) if event.topic == Topic::keyed("agent", &public_id) => {
                        changed = true
                    }
                    Reply::Error { reason, .. } => panic!("prompt failed: {reason}"),
                    _ => {}
                }
            }
            live.wait_for(|live| {
                live.status() == agent::Status::Idle
                    && live
                        .transcript(20)
                        .0
                        .iter()
                        .any(|entry| entry.text == "received once")
            })
            .await
            .unwrap();
        })
        .await
        .unwrap();
        assert_eq!(
            live.borrow()
                .transcript(20)
                .0
                .iter()
                .filter(|entry| entry.text == "received once")
                .count(),
            1
        );
        drop(tx);
        task.await.unwrap().unwrap();
        assert_ne!(session.info().state.status, agent::Status::Ended);
        session.close().await;
    }
}
