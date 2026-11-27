use super::kinds::launch;
use crate::agents::Parts;
use super::*;
use crate::known::KNOWN;
use crate::machine::env::locate::{folder, home_twins};
use super::session::Command;
use crate::logs::picture::Picture;
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{self, CheckoutPlan, SessionsReply, Status},
    harness::AgentSettings,
};
use beep_proto::watched;
use std::{collections::{BTreeMap, HashMap}, path::Path, time::Duration};

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
        bridge: None,
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
    Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::at(root), Default::default()),
        ..Parts::default()
    })
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
        kinds(
            agents
                .transcript_reply(&SessionId::from(CLAUDE_ID), 10, true, None)
                .await
        ),
        [
            (agent::EntryKind::User, "is 391 prime?".into(), None),
            (agent::EntryKind::Thought, "Try 17.".into(), Some(4000)),
            (agent::EntryKind::Agent, "No.".into(), None),
        ]
    );
    assert_eq!(
        kinds(
            agents
                .transcript_reply(&SessionId::from(CLAUDE_ID), 10, false, None)
                .await
        )
        .len(),
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
        .await
        .expect("the session starts");
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| {
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
        bridge: None,
    };
    let begin = Begin {
        resume: Some(CLAUDE_ID.into()),
        ..Begin::default()
    };
    let session = agents
        .start(&kind, Some(dir.to_str().unwrap()), begin)
        .await
        .unwrap();
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| {
            matches!(live.status(), Status::Idle | Status::Ended)
        }),
    )
    .await
    .expect("the session loads")
    .unwrap();
    let key = image_key(&shot);
    let (entries, _) = watched::read(&session.live, |live| live.transcript(10));
    assert_eq!(
        entries[0].text,
        format!("what is this\n[image: image-{key}]\n")
    );
    let served = agents.listings.image(&session.id, &key).await.unwrap();
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
    let found = agents.listings.image(&session.id, &key).await.unwrap();
    assert_eq!(
        found.map(|picture| picture.data.clone()),
        Some(shot.clone())
    );
    assert!(agents.images.get(&session.id, &key).is_some(), "kept again");
    // A session started outside Beep is read from its file by its id.
    let outside = agents
        .listings.image(&SessionId::from(CLAUDE_ID), &key)
        .await
        .unwrap();
    assert_eq!(outside.map(|picture| picture.data.clone()), Some(shot));
    let missing = agents
        .listings.image(&SessionId::from(CLAUDE_ID), &image_key(b"other"))
        .await;
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
    // Claude Code is switched to bypassPermissions by the session, keeps
    // its task tools, and shows its thinking as summaries, unless the
    // node's own environment already chose how.
    let thinking = std::env::var_os("CLAUDE_CODE_THINKING_DISPLAY")
        .is_none()
        .then(|| pair("CLAUDE_CODE_THINKING_DISPLAY", "summarized"));
    let tasks = pair("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1");
    assert_eq!(
        launched("claude"),
        (vec![], std::iter::once(tasks).chain(thinking).collect())
    );
    assert_eq!(
        launched("codex"),
        (
            vec![],
            vec![pair("INITIAL_AGENT_MODE", "agent-full-access")]
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
        launched("agy"),
        (
            vec!["acp".into(), "--dangerously-skip-permissions".into()],
            vec![]
        )
    );
    assert_eq!(
        crate::known::known("hermes").unwrap().bypass_mode,
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
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let begin = || Begin {
        resume: Some(SessionId::from(id)),
        ..Begin::default()
    };
    let first = agents.start(&kind, Some("/"), begin()).await.unwrap();
    let second = agents.start(&kind, Some("/"), begin()).await.err().unwrap();
    assert!(
        second
            .to_string()
            .contains(&format!("already runs here as {}", first.id)),
        "{second}"
    );
    assert_eq!(agents.sessions.owned().get(id), Some(&first.id));
}

#[cfg(unix)]
#[tokio::test]
async fn an_ended_node_session_resumes_from_the_agents_session() {
    let kind = sh_agent("claude", "cat > /dev/null");
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let begin = Begin {
        resume: Some(SessionId::from(id)),
        ..Begin::default()
    };
    let first = agents.start(&kind, Some("/"), begin).await.unwrap();
    first.close().await;
    // Nothing on this machine has the agent's session, so it is the
    // agent's id that the node looks for, not its own.
    let err = agents
        .resume(&kind, &first.id, None, None)
        .await
        .err()
        .unwrap();
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
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
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
        .await
        .unwrap();
    let mut live = first.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| {
            live.status() == Status::Idle || live.status() == Status::Ended
        }),
    )
    .await
    .unwrap()
    .unwrap();
    first.close().await;
    let resumed = agents
        .resume(
            &kind,
            &SessionId::from("hermes-session"),
            Some("continue".into()),
            None,
        )
        .await
        .unwrap();
    let mut live = resumed.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| {
            live.info.turns == 1 || live.status() == Status::Ended
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(resumed.info().turns, 1, "{:?}", resumed.info().state.error);
    assert_eq!(resumed.info().cwd, first.info().cwd);
    assert_eq!(resumed.info().mode.as_deref(), Some("accept_edits"));
    assert!(
        watched::read(&resumed.live, |live| live.transcript(10))
            .0
            .iter()
            .any(|entry| entry.text == "Resumed.")
    );
    resumed.close().await;
    assert!(
        agents
            .resume(&kind, &SessionId::from("outside-session"), None, None)
            .await
            .is_err()
    );
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
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let begin = Begin::default();
    let session = agents.start(&kind, Some("/"), begin).await.unwrap();
    assert!(session.info().id.is_empty());
    assert!(
        agents.sessions.list().is_empty(),
        "ACP has not named this session yet"
    );
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| live.agent_session.is_some()),
    )
    .await
    .expect("the agent gives its id")
    .expect("the session is still there");
    assert_eq!(session.info().id, AGENT_ID);
    assert_eq!(agents.sessions.list()[0].id, AGENT_ID);
    assert!(Arc::ptr_eq(
        &agents.sessions.get(&SessionId::from(AGENT_ID)).unwrap(),
        &session
    ));
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
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let first = agents
        .start(&kind, Some("/"), Begin::default())
        .await
        .unwrap();
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
        .await
        .unwrap();
    let mut live = resumed.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| live.status() == Status::Idle),
    )
    .await
    .expect("the resumed session loads")
    .unwrap();
    let listed = agents.sessions.list();
    assert_eq!(
        listed.len(),
        1,
        "the ended run must not shadow the resumed one"
    );
    assert_eq!(listed[0].id, AGENT_ID);
    assert_eq!(listed[0].state.status, Status::Idle);
    assert!(agents.sessions.get(&first.id).is_none());
    let current = agents.sessions.get(&SessionId::from(AGENT_ID)).unwrap();
    assert!(Arc::ptr_eq(&current, &resumed));
    current
        .order(Command::Prompt("Continue the work".to_owned().into()))
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| {
            live.info.turns == 1 && live.status() == Status::Idle
        }),
    )
    .await
    .expect("the prompt reaches the resumed run")
    .unwrap();
    assert!(
        watched::read(&resumed.live, |live| live.transcript(10))
            .0
            .iter()
            .any(|entry| {
                entry.kind == agent::EntryKind::User && entry.text == "Continue the work"
            })
    );
    resumed.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_refused_start_makes_no_branch() {
    let (_dir, work) = checkout::tests::repo("refused");
    let kind = sh_agent("codex", "cat > /dev/null");
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    agents.catalog.set_settings(AgentSettings {
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
async fn a_start_git_sync_holds_back_makes_no_worktree() {
    let (_dir, work) = checkout::tests::repo("held");
    let kind = sh_agent("codex", "cat > /dev/null");
    let agents = Arc::new(Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() }));
    let _hold = agents.occupancy.holds().hold(&work).await.unwrap();
    let plan = CheckoutPlan::NamedWorktree {
        base: None,
        branch: "issue-7-fix".to_owned(),
    };
    let begin = Begin {
        prompt: Some("Work on issue #7".into()),
        ..Begin::default()
    };
    let refused = agents
        .start_in(&kind, work.to_str(), Some(plan), begin)
        .await
        .err();

    assert!(refused.is_some_and(|e| e.to_string().contains("preserving local edits")));
    let trees = checkout::tests::run(&work, &["worktree", "list", "--porcelain"]);
    assert_eq!(trees.matches("worktree ").count(), 1);
    assert!(checkout::tests::run(&work, &["branch", "--list", "issue-7*"]).is_empty());
}

/// An issue's session is known by the agent's id and its worktree, whose
/// branch is the issue's.
#[cfg(unix)]
#[tokio::test]
async fn an_issue_session_reports_its_agents_id_and_worktree() {
    const AGENT_ID: &str = "0199b2c3-4d5e-7f60-8a9b-0c1d2e3f4a5b";
    let (_dir, work) = checkout::tests::repo("issue");
    let agents = Agents::new(Parts { kinds: vec![sh_agent("codex", NAMING_ADAPTER)], ..Parts::default() });
    let plan = CheckoutPlan::NamedWorktree {
        base: None,
        branch: "issue-7-fix-it".to_owned(),
    };
    let started = agents.start_on_issue(
        work.to_str().unwrap(),
        plan,
        "#7 Fix it".to_owned(),
        "Work on issue #7".to_owned(),
    );
    let (id, worktree) = tokio::time::timeout(Duration::from_secs(10), started)
        .await
        .expect("the agent names the session")
        .unwrap();

    assert_eq!(id, AGENT_ID);
    assert_ne!(Path::new(&worktree), work);
    assert_eq!(
        crate::team::pickup::branch_of(&worktree).await.as_deref(),
        Some("issue-7-fix-it")
    );
    agents.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_resumes_only_on_its_own_agent() {
    let kind = |name: &str| sh_agent(name, "cat > /dev/null");
    let (claude, codex) = (kind("claude"), kind("codex"));
    let agents = Agents::new(Parts { kinds: vec![claude.clone(), codex.clone()], ..Parts::default() });
    let begin = Begin {
        resume: Some(SessionId::from("0199a1b2-3c4d-7e5f-8a9b-0c1d2e3f4a5b").to_owned()),
        ..Begin::default()
    };
    let running = agents.start(&codex, Some("/"), begin).await.unwrap();
    let err = agents.resume(&claude, &running.id, None, None).await.err();
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
    let agents = Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::locate_in(root.path()), Default::default()),
        ..Parts::default()
    });
    assert_eq!(
        agents.catalog.offers(),
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
    let listed = agents.listings.external().list(10, None, HashMap::new()).await;
    assert_eq!(listed.expect("a list").len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn a_live_view_survives_idle_time_and_detaching_keeps_the_agent_running() {
    use agent::{FollowSession, SessionUpdate};
    use tokio::time::{advance, pause, resume, timeout};
    let root = tempfile::tempdir().unwrap();
    let agents = Arc::new(Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(Default::default(), crate::logs::found::Stores::default()),
        ..Parts::default()
    }));
    let kind = sh_agent("codex", THINKING_ADAPTER);
    let session = agents
        .start(&kind, root.path().to_str(), Begin::default())
        .await
        .unwrap();
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| live.status() == Status::Idle),
    )
    .await
    .unwrap()
    .unwrap();
    let request = FollowSession {
        agent: "codex".into(),
        session: Into::<SessionId>::into(session.info().id),
        last: 200,
        after: None,
        patches: false,
        shown: true,
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
        watched::wait_for(&mut live, |live| live.info.turns >= 2),
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
            bridge: None,
        };
        let agents = Arc::new(Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() }));
        let session = agents
            .start(&kind, root.path().to_str(), Begin::default())
            .await
            .unwrap();
        let public_id = session.named_info().await.unwrap().id;
        let mut live = session.live.clone();
        timeout(
            Duration::from_secs(5),
            watched::wait_for(&mut live, |live| live.status() == agent::Status::Idle),
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

            watched::wait_for(&mut live, |live| {
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
            watched::read(&live, |live| live.transcript(20))
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

#[tokio::test]
async fn project_history_pages_metadata_without_preview_text() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions/--project--");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::create_dir_all(root.path().join("projects")).unwrap();
    for i in 0..125 {
        let text = format!(
            "{{\"type\":\"session\",\"id\":\"session-{i:03}\",\"cwd\":\"/srv/project\",\"timestamp\":\"2026-01-01T00:00:00Z\"}}\n{{\"type\":\"session_info\",\"name\":\"A session title\"}}\n"
        );
        std::fs::write(sessions.join(format!("session-{i:03}.jsonl")), text).unwrap();
    }
    let agents = Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::at(root.path()), crate::logs::found::Stores::at(None, Some(&root.path().join("sessions")), None)),
        ..Parts::default()
    });
    let mut cursor = None;
    let mut ids = std::collections::HashSet::new();
    loop {
        let page = agents
            .listings.sessions_page(vec!["/srv/project".into()], 50, cursor)
            .await
            .unwrap();
        assert!(page.sessions.len() <= 50);
        for session in page.sessions {
            assert_eq!(session.title.as_deref(), Some("A session title"));
            assert!(session.last_prompt.is_empty());
            assert!(session.last_message.is_empty());
            assert!(ids.insert(session.id));
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 125);
}

#[cfg(unix)]
#[tokio::test]
async fn owned_claude_sessions_report_the_native_cwd_after_relative_shell_edits() {
    use super::checkout::tests::run;
    use std::io::Write;

    let root = tempfile::tempdir().expect("a temp folder");
    let repo = root.path().join("project");
    std::fs::create_dir(&repo).expect("a checkout folder");
    run(&repo, &["init", "-q", "-b", "main"]);
    run(&repo, &["commit", "-q", "--allow-empty", "-m", "start"]);
    run(
        &repo,
        &["worktree", "add", "-q", "-b", "fix", ".worktrees/fix"],
    );
    let tree = repo.join(".worktrees/fix");
    let projects = root
        .path()
        .join("claude/projects")
        .join(crate::logs::claude_dir::slug(&repo));
    std::fs::create_dir_all(&projects).expect("a transcript folder");
    let path = projects.join(format!("{CLAUDE_ID}.jsonl"));
    let append = |cwd: &Path| {
        let line = serde_json::json!({
            "type": "assistant", "sessionId": CLAUDE_ID, "cwd": cwd,
            "message": {"content": [{"type": "tool_use", "id": "t1", "name": "Bash",
                "input": {"command": "python3 edit.py"}}]}
        });
        writeln!(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap(),
            "{line}"
        )
        .unwrap();
    };
    append(&repo);
    let agents = agents_at(&root.path().join("claude"));
    let script = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply '{"sessionId":"SESSION_ID"}'
while read -r line; do :; done
"#
    .replace("SESSION_ID", CLAUDE_ID);
    let session = agents
        .start(
            &sh_agent("claude", &script),
            Some(repo.to_str().unwrap()),
            Begin::default(),
        )
        .await
        .unwrap();
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| live.agent_session.is_some()),
    )
    .await
    .expect("the session starts")
    .expect("still running");

    append(&tree);
    let listed = agents.listings.listed_with_places(false).await;
    let place = listed[0].place.as_ref().expect("a reported checkout");
    // macOS temp folders sit behind a `/var` link that `paths::canonicalize` resolves.
    let tree = tree.canonicalize().expect("the linked checkout");
    assert_eq!(place.folder, tree.to_string_lossy());
    assert!(place.linked);
    assert_eq!(place.branch.as_deref(), Some("fix"));
    assert_eq!(listed[0].cwd, repo.to_string_lossy());

    append(&repo);
    let listed = agents.listings.listed_with_places(false).await;
    assert!(!listed[0].place.as_ref().unwrap().linked);
    session.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn native_places_skip_an_unreadable_transcript() {
    use super::checkout::tests::run;
    use std::collections::HashSet;

    const BROKEN: &str = "7a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";
    let root = tempfile::tempdir().expect("a temp folder");
    let repo = root.path().join("project");
    std::fs::create_dir(&repo).expect("a checkout folder");
    run(&repo, &["init", "-q", "-b", "main"]);
    let config = root.path().join("claude");
    let folder = config.join("projects").join(crate::logs::claude_dir::slug(&repo));
    std::fs::create_dir_all(&folder).expect("a transcript folder");
    let line = serde_json::json!({
        "type": "assistant", "cwd": repo,
        "message": {"content": [{"type": "text", "text": "Done."}]}
    });
    std::fs::write(
        folder.join(format!("{CLAUDE_ID}.jsonl")),
        format!("{line}\n"),
    )
    .expect("the valid transcript");
    // A directory with a transcript's name cannot be read as a transcript.
    std::fs::create_dir(folder.join(format!("{BROKEN}.jsonl"))).expect("an unreadable transcript");

    let places = crate::logs::claude::Store::at(&config)
        .places(HashSet::from([
            SessionId::from(CLAUDE_ID),
            SessionId::from(BROKEN),
            SessionId::from("../outside"),
        ]))
        .await
        .expect("native places remain available");

    assert_eq!(places.len(), 1);
    let repo = repo.canonicalize().expect("the checkout");
    assert_eq!(
        places[&SessionId::from(CLAUDE_ID)].folder,
        repo.to_string_lossy()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn resumed_claude_reads_native_history_before_a_partial_replay() {
    use beep_proto::node::agent::EntryKind;

    for compacted in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("claude");
        let folder = config.join("projects/project");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("{CLAUDE_ID}.jsonl"));
        let mut records = vec![
            serde_json::json!({"type":"user", "uuid":"old-user", "message":{"content":"yes"}}),
            serde_json::json!({"type":"assistant", "uuid":"old-agent", "parentUuid":"old-user", "message":{"content":[{"type":"text", "text":"Original answer"}]}}),
        ];
        let summary = "This session is being continued from a previous conversation that ran out of context. Keep the literal <beep-context>example</beep-context> in the summary.";
        if compacted {
            records.push(serde_json::json!({"type":"system", "subtype":"compact_boundary"}));
            records.push(serde_json::json!({"type":"user","isCompactSummary":true,"message":{"content":"Older summary"}}));
            records.push(serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"Intermediate reply"}]}}));
            records.push(serde_json::json!({"type":"system", "subtype":"compact_boundary"}));
        }
        records.extend([
            serde_json::json!({"type":"user", "uuid":"new-user", "parentUuid":"missing", "isCompactSummary":compacted, "message":{"content":if compacted {summary} else {"yes"}}}),
            serde_json::json!({"type":"assistant", "uuid":"new-agent", "parentUuid":"new-user", "message":{"content":[{"type":"text", "text":"Recent answer"}]}}),
        ]);
        let text = records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>();
        std::fs::write(&path, &text).unwrap();
        let agents = agents_at(&config);
        let script = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
update() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"%s","content":{"type":"text","text":"%s"}}}}\n' "$1" "$2"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
read -r line
update user_message_chunk yes
update agent_message_chunk 'Recent answer'
reply '{}'
while read -r line; do
  case "$line" in
    *session/prompt*) update agent_message_chunk 'New answer'; reply '{"stopReason":"end_turn"}' ;;
    *) reply '{}' ;;
  esac
done
"#;
        let script = if compacted {
            script.replace("update user_message_chunk yes", &format!("update user_message_chunk '<beep-context>Context</beep-context>'\nupdate user_message_chunk '{summary}'"))
        } else {
            script.to_owned()
        };
        let session = agents
            .start(
                &sh_agent("claude", &script),
                Some(root.path().to_str().unwrap()),
                Begin {
                    resume: Some(CLAUDE_ID.into()),
                    ..Begin::default()
                },
            )
            .await
            .unwrap();
        let mut live = session.live.clone();
        tokio::time::timeout(
            Duration::from_secs(10),
            watched::wait_for(&mut live, |live| live.status() == Status::Idle),
        )
        .await
        .unwrap()
        .unwrap();

        let expected = if compacted {
            super::compaction::summary_note(summary)
        } else {
            "yes".to_owned()
        };
        let (recent, earlier) = agents.live_transcript(&session, 2, None).await;
        assert_eq!(
            recent
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            [expected.as_str(), "Recent answer"]
        );
        assert_eq!(earlier, if compacted { 4 } else { 2 });
        let (older, start) = agents.live_transcript(&session, 20, Some(earlier)).await;
        assert_eq!(start, 0);
        assert_eq!(older[0].text, "yes");
        assert_eq!(older[1].text, "Original answer");
        assert_eq!(older.len(), earlier as usize);
        if compacted {
            assert_eq!(older[2].kind, EntryKind::Compacted);
            assert_eq!(older[2].text, "[conversation compacted]\nOlder summary");
            assert_eq!(older[3].text, "Intermediate reply");
            assert_eq!(recent[0].kind, EntryKind::Compacted);
        }
        let (joined, start) = agents.live_transcript(&session, 20, None).await;
        assert_eq!(start, 0);
        assert_eq!(joined.len(), older.len() + recent.len());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            text,
            "native history stays intact"
        );

        session
            .order(Command::Prompt("Continue".to_owned().into()))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(10),
            watched::wait_for(&mut live, |live| live.info.turns == 1),
        )
        .await
        .unwrap()
        .unwrap();
        let (updated, start) = agents.live_transcript(&session, 20, None).await;
        assert_eq!(start, 0);
        assert_eq!(&updated[..joined.len()], joined);
        assert_eq!(updated.last().unwrap().text, "New answer");
        session.close().await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_continued_session_inherits_the_earlier_title_over_agent_renames() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let agents = Agents::default();
    let renaming = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"next-1"}'
while read -r line; do
  case "$line" in
    *session/prompt*)
      echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"next-1","update":{"sessionUpdate":"session_info_update","title":"Status"}}}'
      echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"next-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Checked."}}}}'
      reply '{"stopReason":"end_turn"}' ;;
    *) reply '{}' ;;
  esac
done
"#;
    let first_kind = sh_agent("pi", THINKING_ADAPTER);
    let earlier = agents
        .start(
            &first_kind,
            Some(dir.path().to_str().unwrap()),
            Begin {
                prompt: Some("Continue work on direct S3 ingestion".into()),
                title: Some("Continue work on direct S3 ingestion".into()),
                ..Begin::default()
            },
        )
        .await
        .expect("earlier session starts");
    let mut earlier_live = earlier.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut earlier_live, |live| live.status() == Status::Idle),
    )
    .await
    .unwrap()
    .unwrap();

    let next_kind = sh_agent("flsh", renaming);
    let continued = agents
        .start(
            &next_kind,
            Some(dir.path().to_str().unwrap()),
            Begin {
                prompt: Some("status".into()),
                continues: Some(earlier.id.clone()),
                ..Begin::default()
            },
        )
        .await
        .expect("continued session starts");
    let mut next_live = continued.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut next_live, |live| {
            live.status() == Status::Idle && live.info.turns == 1
        }),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(
        continued.info().title.as_deref(),
        Some("Continue work on direct S3 ingestion")
    );
    assert_eq!(continued.info().continues.as_ref(), Some(&earlier.id));
    earlier.close().await;
    continued.close().await;
}
