use super::*;
use crate::agents::{child_reach::show_children, family::peer};
use serde_json::json;
use crate::{
    agents::{AgentKind, Begin},
};
use beep_proto::AgentSlug;
use beep_proto::node::agent::{Entry, EntryKind, Question, SessionInfo, State, Status, WorkPlace};
use serde_json::Value;
use beep_proto::watched;
use std::{collections::BTreeMap, time::Duration};

fn endpoint() -> EndpointId {
    iroh::SecretKey::generate().public()
}

#[tokio::test]
async fn a_stop_is_sent_again_until_the_machine_answers_and_then_gives_up() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let sent = AtomicU32::new(0);
    // The machine is back on the third try.
    let up = peer::retrying(4, Duration::from_millis(1), || async {
        if sent.fetch_add(1, Ordering::Relaxed) < 2 {
            Err("did not answer".to_owned())
        } else {
            Ok(())
        }
    })
    .await;
    assert_eq!((up, sent.load(Ordering::Relaxed)), (Ok(()), 3));
    // One that never is, is tried a bounded number of times.
    sent.store(0, Ordering::Relaxed);
    let down = peer::retrying(4, Duration::from_millis(1), || async {
        sent.fetch_add(1, Ordering::Relaxed);
        Err::<(), _>("did not answer".to_owned())
    })
    .await;
    assert_eq!((down.is_err(), sent.load(Ordering::Relaxed)), (true, 4));
}

#[test]
fn a_child_is_started_in_the_same_project_on_the_other_machine() {
    let (me, there) = (endpoint(), endpoint());
    let spec = SyncSpec {
        ignored: Default::default(),
        folders: [
            (me, "/srv/game".to_owned()),
            (there, "/Users/box/game".to_owned()),
        ]
        .into(),
        paused: false,
        rules: String::new(),
    };
    assert_eq!(
        folder_on(
            std::slice::from_ref(&spec),
            me,
            Path::new("/srv/game/crates/core"),
            there
        )
        .as_deref(),
        Some("/Users/box/game/crates/core")
    );
    assert_eq!(folder_on(&[spec], me, Path::new("/srv/other"), there), None);
}

#[test]
fn what_an_agent_reads_of_a_child_says_when_it_waits_on_the_user() {
    let info = |status, turns, question: Option<&str>| SessionInfo {
        id: "c1".into(),
        agent: "codex".into(),
        turns,
        last_message: "Done.".into(),
        question: question.map(|text| Question {
            id: 1,
            text: text.into(),
            detail: None,
            options: Vec::new(),
        }),
        state: State {
            status,
            files_touched: vec!["a.rs".into(), "b.rs".into()],
            ..State::default()
        },
        ..SessionInfo::default()
    };
    let view = |info: &SessionInfo| ChildView::of(info, &SessionId::from("c1")).named("build-box");
    let working = view(&info(Status::Working, 0, None));
    assert_eq!(working.state, "working");
    assert_eq!(working.id, "c1@build-box");
    assert!(!working.settled());
    // Starting is working; a resumed session that waits for a prompt is idle
    // though its turn count started over.
    assert_eq!(view(&info(Status::Starting, 0, None)).state, "working");
    assert_eq!(view(&info(Status::Idle, 0, None)).state, "idle");
    let done = view(&info(Status::Idle, 1, None));
    assert_eq!((done.state.as_str(), done.files_changed), ("idle", 2));
    assert_eq!(done.last_message, "Done.");
    let asking = view(&info(Status::Asking, 1, Some("Run cargo test")));
    assert_eq!(asking.state, "waiting_on_user");
    assert_eq!(asking.question.as_deref(), Some("Run cargo test"));
    assert_eq!(view(&info(Status::Ended, 1, None)).state, "ended");
    // Naming a machine twice does not double it.
    assert_eq!(done.named("build-box").id, "c1@build-box");
}

#[test]
fn another_machine_reads_the_branch_a_session_works_on_from_its_listing() {
    let info = SessionInfo {
        id: "c1".into(),
        place: Some(WorkPlace {
            branch: Some("issue-52-fix".into()),
            ..WorkPlace::default()
        }),
        ..SessionInfo::default()
    };
    let listed = serde_json::to_string(&[ChildView::of(&info, &SessionId::from("c1"))]).unwrap();
    let read: Vec<ChildView> = serde_json::from_str(&listed).unwrap();
    assert_eq!(read[0].branch.as_deref(), Some("issue-52-fix"));

    // An older machine lists no branch.
    let older: Vec<ChildView> = serde_json::from_str(r#"[{"id":"c1","harness":"codex","machine":"","state":"idle","title":null,"last_message":"","files_changed":0,"question":null,"turns":0,"tokens":0,"cost_usd":null,"error":null}]"#).unwrap();
    assert_eq!(older[0].branch, None);
}

/// An ACP adapter in shell: it answers each prompt at once, except one that
/// starts with `hang`, which ends when the turn is cancelled.
const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
while read -r line; do
  case "$line" in
    *'"initialize"'*) reply '{"protocolVersion":1,"agentCapabilities":{}}' ;;
    *'"session/new"'*) reply '{"sessionId":"s-'$$'"}' ;;
    *'"text":"hang'*) prompt=$(id) ;;
    *'"session/cancel"'*) [ -n "$prompt" ] && printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt" ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"id"'*) reply '{}' ;;
  esac
done
"#;

fn kind(name: &str) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec!["-c".into(), ADAPTER.into(), "fake-adapter".into()],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

async fn tool(node: &Node, session: &SessionId, tool: Tool, args: Value) -> (bool, String) {
    let request = Request {
        session: session.clone(),
        tool,
        args: args.to_string(),
        approved: Default::default(),
    };

    match call(node, &request).await {
        Reply::Done { text, failed } => (failed, text),
        other => panic!("not done: {other:?}"),
    }
}

fn view(text: &str) -> ChildView {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

#[cfg(unix)]
#[tokio::test]
async fn a_project_child_uses_its_checkout_and_accepts_followups() {
    use beep_mesh::Key;
    use beep_proto::node::mesh_state::{Place, ProjectRecord, StateValue};
    use iroh::{Endpoint, endpoint::presets};

    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint.clone(), "build-box")
        .await
        .unwrap();
    let checkout = dir.path().join("app");
    std::fs::create_dir(&checkout).unwrap();
    mesh.set(
        Key::Project(format!("example/app/{}", mesh.me())),
        Some(
            Place {
                folder: checkout.display().to_string(),
                imported_ms: 0,
                fork: None,
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    let mut node = Node::test(dir.path()).await;
    node.agents = Arc::new(Agents::new(vec![kind("top"), kind("kid")], crate::occupancy::Occupancy::default()));
    node.mesh.set(mesh);
    node.agents.serve_mesh(node.mesh.clone());
    let parent = node
        .agents
        .start(
            &node.agents.kinds()[0],
            Some(dir.path().to_str().unwrap()),
            Begin {
                prompt: Some("hello".into()),
                ..Begin::default()
            },
        )
        .await.unwrap();
    let mut live = parent.live.clone();
    tokio::time::timeout(
        Duration::from_secs(20),
        watched::wait_for(&mut live, |live| {
            live.agent_session.is_some() && live.info.turns == 1
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let parent_id = Agents::name_of(&parent);

    for args in [
        json!({"harness":"kid", "prompt":"work", "project":"missing"}),
        json!({"harness":"kid", "prompt":"work", "project":"example/app", "cwd":"/srv/other"}),
    ] {
        let (failed, text) = tool(&node, &parent_id, Tool::StartAgent, args).await;
        assert!(failed, "{text}");
    }

    // A project limited to one harness refuses a session of another.
    let limit = |harnesses: Option<Vec<AgentSlug>>| {
        let mesh = node.mesh.get().unwrap();
        async move {
            mesh.set(
                Key::Project("example/app".to_owned()),
                Some(
                    ProjectRecord {
                        harnesses,
                        ..ProjectRecord::default()
                    }
                    .to_bytes()
                    .unwrap(),
                ),
            )
            .await
            .unwrap();
        }
    };
    limit(Some(vec![AgentSlug::new("top")])).await;
    let (failed, text) = tool(
        &node,
        &parent_id,
        Tool::StartAgent,
        json!({"harness":"kid", "prompt":"work", "project":"example/app"}),
    )
    .await;
    assert!(failed && text.contains("allows only top"), "{text}");
    limit(Some(vec![AgentSlug::new("top"), AgentSlug::new("kid")])).await;

    let (failed, text) = tool(
        &node,
        &parent_id,
        Tool::StartAgent,
        json!({"harness":"kid", "prompt":"work", "project":"example/app", "wait":true}),
    )
    .await;
    assert!(!failed, "{text}");
    let done = view(&text);
    let who = node.agents.caller(&parent_id).unwrap().1;
    let child = node
        .agents
        .child_of(&who, &ChildId::from(done.id.as_str()).session)
        .unwrap();
    assert_eq!(child.info().cwd, checkout.display().to_string());
    let mut child_live = child.live.clone();
    let (failed, text) = tool(
        &node,
        &parent_id,
        Tool::SendToAgent,
        json!({"id":done.id, "text":"continue"}),
    )
    .await;
    assert!(!failed, "{text}");
    tokio::time::timeout(
        Duration::from_secs(20),
        watched::wait_for(&mut child_live, |live| {
            live.info.turns == 2 && live.info.state.status == Status::Idle
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let (failed, text) = tool(
        &node,
        &parent_id,
        Tool::AgentStatus,
        json!({"id":done.id}),
    )
    .await;
    assert!(!failed, "{text}");
    assert_eq!(view(&text).turns, 2);
    node.agents.shutdown().await;
    endpoint.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_thread_runs_on_its_chats_harness_unless_it_names_one() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let mut node = Node::test(dir.path()).await;
    node.agents = Arc::new(Agents::new(vec![kind("top"), kind("kid")], crate::occupancy::Occupancy::default()));
    let cwd = dir.path().to_string_lossy().into_owned();
    let chat = node
        .agents
        .start(
            &node.agents.kinds()[0],
            Some(&cwd),
            Begin {
                prompt: Some("hello".to_owned()),
                ..Begin::default()
            },
        )
        .await.expect("the chat starts");
    let mut live = chat.live.clone();
    tokio::time::timeout(
        Duration::from_secs(20),
        watched::wait_for(&mut live, |live| {
            live.agent_session.is_some() && live.info.turns == 1
        }),
    )
    .await
    .expect("the chat settles")
    .expect("the chat is kept");
    let chat = Agents::name_of(&chat);

    // `start_thread` calls `start_agent` with what it takes: no harness.
    let (failed, text) = tool(
        &node,
        &chat,
        Tool::StartAgent,
        json!({ "title": "Pricing", "prompt": "work" }),
    )
    .await;
    assert!(!failed, "{text}");
    let thread = view(&text);
    assert_eq!(
        (thread.harness.as_ref(), thread.title.as_deref()),
        ("top", Some("Pricing"))
    );
    let (failed, text) = tool(
        &node,
        &chat,
        Tool::StartAgent,
        json!({ "title": "Legal", "prompt": "work", "harness": "kid" }),
    )
    .await;
    assert!(!failed, "{text}");
    assert_eq!(view(&text).harness, "kid");
    node.agents.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_thread_runs_as_a_persona_on_its_harness_when_this_machine_has_it() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let write = |rel: &str, text: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("folder made");
        std::fs::write(path, text).expect("file written");
    };
    write(
        ".codex/agents/researcher.toml",
        "name = \"researcher\"\ndeveloper_instructions = \"Cite sources.\"",
    );
    write(".claude/agents/writer.md", "---\nname: writer\n---\nWrite plainly.");
    let mut node = Node::test(dir.path()).await;
    node.agents = Arc::new(Agents::new(vec![kind("top"), kind("codex")], crate::occupancy::Occupancy::default()));
    let cwd = dir.path().to_string_lossy().into_owned();
    let chat = node
        .agents
        .start(
            &node.agents.kinds()[0],
            Some(&cwd),
            Begin {
                prompt: Some("hello".to_owned()),
                ..Begin::default()
            },
        )
        .await.expect("the chat starts");
    let mut live = chat.live.clone();
    tokio::time::timeout(
        Duration::from_secs(20),
        watched::wait_for(&mut live, |live| {
            live.agent_session.is_some() && live.info.turns == 1
        }),
    )
    .await
    .expect("the chat settles")
    .expect("the chat is kept");
    let chat = Agents::name_of(&chat);
    let start = |persona: &str, harness: Option<&str>| {
        let mut args = json!({ "title": persona, "prompt": "work", "persona": persona });
        if let Some(harness) = harness {
            args["harness"] = json!(harness);
        }
        tool(&node, &chat, Tool::StartAgent, args)
    };

    // Its own harness, which this machine has.
    let (failed, text) = start("researcher", None).await;
    assert!(!failed, "{text}");
    let researcher = view(&text);
    assert_eq!(researcher.harness, "codex");
    let session = node.agents.get(&researcher.key).expect("the thread runs here");
    assert_eq!(session.info().persona.map(|persona| persona.name).as_deref(), Some("researcher"));
    // Claude's, which it has not: the chat's.
    let (failed, text) = start("writer", None).await;
    assert!(!failed, "{text}");
    assert_eq!(view(&text).harness, "top");
    // The one named.
    let (failed, text) = start("Researcher", Some("top")).await;
    assert!(!failed, "{text}");
    assert_eq!(view(&text).harness, "top");

    let (failed, text) = start("nobody", None).await;
    assert!(failed);
    assert!(text.starts_with("There is no custom agent called nobody. There are: writer, researcher"), "{text}");
    node.agents.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_starts_follows_messages_and_stops_its_own_child() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let mut node = Node::test(dir.path()).await;
    node.agents = Arc::new(Agents::new(vec![kind("top"), kind("kid")], crate::occupancy::Occupancy::default()));
    let cwd = dir.path().to_string_lossy().into_owned();
    let start = async |name: &str| {
        node
            .agents
            .start(
                &node.agents.kinds()[0],
                Some(&cwd),
                Begin {
                    prompt: Some(format!("hello {name}")),
                    ..Begin::default()
                },
            )
            .await.expect("a session starts")
    };
    let (mine, theirs) = (start("mine").await, start("theirs").await);

    for session in [&mine, &theirs] {
        let mut live = session.live.clone();
        tokio::time::timeout(
            Duration::from_secs(20),
            watched::wait_for(&mut live, |live| {
                live.agent_session.is_some() && live.info.turns == 1
            }),
        )
        .await
        .expect("the session settles")
        .expect("the session is kept");
    }

    let (mine, theirs) = (Agents::name_of(&mine), Agents::name_of(&theirs));

    // `wait` returns the answer once the child's turn ends.
    let (failed, text) = tool(
        &node,
        &mine,
        Tool::StartAgent,
        json!({ "harness": "kid", "prompt": "work", "wait": true }),
    )
    .await;
    assert!(!failed, "{text}");
    let done = view(&text);
    assert_eq!(
        (done.state.as_str(), done.harness.as_ref()),
        ("idle", "kid")
    );
    assert_eq!(done.turns, 1);
    assert!(done.note.as_deref().is_some_and(|note| note.contains("stop_agent")));

    // A child that is still working returns at once without `wait`.
    let (failed, text) = tool(
        &node,
        &mine,
        Tool::StartAgent,
        json!({ "harness": "kid", "prompt": "hang about" }),
    )
    .await;
    assert!(!failed, "{text}");
    let busy = view(&text);
    assert_eq!(busy.state, "working");

    // A child still waiting for ACP counts towards limits but has no id to list.
    let (parent, who) = node.agents.caller(&mine).unwrap();
    let mut unnamed_kind = kind("kid");
    unnamed_kind.args = vec!["-c".into(), "cat > /dev/null".into()];
    let unnamed = node
        .agents
        .start(
            &unnamed_kind,
            Some(&cwd),
            Begin {
                parent: Some(node.agents.link_for(&parent, &who).unwrap()),
                ..Begin::default()
            },
        )
        .await.unwrap();
    assert_eq!(node.agents.children_of(&who).len(), 3);
    let (_, text) = tool(&node, &mine, Tool::ListAgents, json!({})).await;
    let listed: Vec<ChildView> = serde_json::from_str(&text).expect("a list");
    assert_eq!(listed.len(), 2, "{text}");
    unnamed.close().await;

    // Another agent neither checks nor stops them, and lists none as its own.
    for tool_name in [Tool::AgentStatus, Tool::StopAgent] {
        let (failed, text) = tool(&node, &theirs, tool_name, json!({ "id": done.id })).await;
        assert!(failed && text.starts_with("You have no agent"), "{text}");
    }

    let (_, text) = tool(&node, &theirs, Tool::ListAgents, json!({})).await;
    assert_eq!(text, "[]");

    // It finds them among every agent and messages one.
    let (_, text) = tool(&node, &theirs, Tool::ListAgents, json!({ "all": true })).await;
    let everyone: Vec<ChildView> = serde_json::from_str(&text).expect("a list");
    assert!(everyone.iter().any(|view| view.id == done.id), "{text}");
    let (failed, text) = tool(
        &node,
        &theirs,
        Tool::SendToAgent,
        json!({ "id": done.id, "text": "hi" }),
    )
    .await;
    assert!(!failed, "{text}");

    // Its own agent takes a message and reads its answer.
    let (failed, text) = tool(
        &node,
        &mine,
        Tool::SendToAgent,
        json!({ "id": done.id, "text": "once more" }),
    )
    .await;
    assert!(!failed, "{text}");
    let (_, text) = tool(&node, &mine, Tool::AgentStatus, json!({ "id": done.id })).await;
    assert!(view(&text).turns >= 1);

    // Stopping one child leaves the other; the parent leaves neither.
    let (failed, _) = tool(&node, &mine, Tool::StopAgent, json!({ "id": busy.id })).await;
    assert!(!failed);
    let (_, text) = tool(&node, &mine, Tool::AgentStatus, json!({ "id": busy.id })).await;
    assert_eq!(view(&text).state, "ended");
    let (_, text) = tool(&node, &mine, Tool::AgentStatus, json!({ "id": done.id })).await;
    assert_ne!(view(&text).state, "ended");

    // A child its parent no longer has shows as ended, not as it started.
    let machine = busy.id.split_once('@').map_or("", |(_, machine)| machine);
    let gone = ChildView {
        id: format!("gone@{machine}"),
        ..busy
    };
    let call = format!(
        "Beep: Start kid agent: hang about [completed]\n{}",
        serde_json::to_string(&gone).unwrap()
    );
    let mut entries = [Entry::new(EntryKind::Tool, call)];
    show_children(&node.agents, &parent, &mut entries);
    assert!(
        entries[0].text.contains(&format!("[child] {} ended", gone.id)),
        "{}",
        entries[0].text
    );
    node.agents.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_in_a_restricted_mode_starts_no_child_of_another_harness() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let mut node = Node::test(dir.path()).await;
    // The harness named `claude` has a full access mode of its own.
    node.agents = Arc::new(Agents::new(vec![kind("claude"), kind("kid")], crate::occupancy::Occupancy::default()));
    let cwd = dir.path().to_string_lossy().into_owned();
    let parent = node
        .agents
        .start(
            &node.agents.kinds()[0],
            Some(&cwd),
            Begin {
                prompt: Some("hello".into()),
                mode: Some("plan".into()),
                ..Begin::default()
            },
        )
        .await.expect("a session starts");
    let mut live = parent.live.clone();
    tokio::time::timeout(
        Duration::from_secs(20),
        watched::wait_for(&mut live, |live| {
            live.agent_session.is_some() && live.info.turns == 1
        }),
    )
    .await
    .expect("the session settles")
    .expect("the session is kept");
    let id = Agents::name_of(&parent);
    let (failed, text) = tool(
        &node,
        &id,
        Tool::StartAgent,
        json!({ "harness": "kid", "prompt": "go" }),
    )
    .await;
    assert!(failed && text.contains("below full access"), "{text}");
    let (failed, text) = tool(
        &node,
        &id,
        Tool::StartAgent,
        json!({ "harness": "claude", "prompt": "go", "wait": true }),
    )
    .await;
    assert!(!failed, "{text}");
    node.agents.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn another_machine_lists_the_sessions_ended_since_only_when_asked() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let agents = Agents::new(vec![kind("top")], crate::occupancy::Occupancy::default());
    let cwd = dir.path().to_string_lossy().into_owned();
    let start = async || {
        let begin = Begin {
            prompt: Some("hello".to_owned()),
            ..Begin::default()
        };
        agents.start(&agents.kinds()[0], Some(&cwd), begin).await.expect("a session starts")
    };
    let (ended, running) = (start().await, start().await);

    for session in [&ended, &running] {
        let mut live = session.live.clone();
        tokio::time::timeout(
            Duration::from_secs(20),
            watched::wait_for(&mut live, |live| live.info.turns == 1),
        )
        .await
        .expect("the session settles")
        .expect("the session is kept");
    }
    ended.close().await;
    let ended_at = ended.info().last_activity;
    let names = |args: &str| {
        let Reply::Done { text, .. } = listed(&agents, args) else {
            panic!("not done");
        };
        let views: Vec<ChildView> = serde_json::from_str(&text).expect("a list");
        let mut states: Vec<String> = views.into_iter().map(|view| view.state).collect();
        // Started in the same second, they list in either order.
        states.sort();
        states
    };

    assert_eq!(names("{}"), ["idle"]);
    assert_eq!(names(&format!(r#"{{"ended_since":{ended_at}}}"#)), ["ended", "idle"]);
    assert_eq!(names(&format!(r#"{{"ended_since":{}}}"#, ended_at + 1)), ["idle"]);
    agents.shutdown().await;
}

fn state(state: &str) -> ChildView {
    ChildView {
        id: "k0@box".into(),
        state: state.into(),
        ..ChildView::default()
    }
}

#[tokio::test(start_paused = true)]
async fn a_wait_on_a_remote_child_of_an_older_node_still_ends_early() {
    let waiting = crate::agents::family::links::Waits::default().hold(&SessionId::from("p0"), "k0@box");
    let reads = std::cell::Cell::new(0);
    let started = tokio::time::Instant::now();
    // The first read is the wait's own; the child finished after it, and no
    // note came.
    let read = |_: String| {
        reads.set(reads.get() + 1);
        let now = if reads.get() < 3 { "working" } else { "idle" };
        std::future::ready(Ok(state(now)))
    };
    let done = wait_for_settled(&waiting, None, true, state("working"), read).await;

    assert_eq!(done.state, "idle");
    assert_eq!(done.note, None);
    assert_eq!(reads.get(), 3);
    assert_eq!(started.elapsed(), WAIT_RECHECK * 2);
}

#[tokio::test(start_paused = true)]
async fn a_wait_on_a_remote_child_that_notes_every_settle_waits_for_its_wake_and_gives_up_at_the_cap() {
    let waiting = crate::agents::family::links::Waits::default().hold(&SessionId::from("p0"), "k0@box");
    let read = |_: String| {
        std::future::ready(Ok(ChildView {
            notes_settles: true,
            ..state("working")
        }))
    };
    let done = wait_for_settled(&waiting, None, true, state("working"), read).await;

    assert!(done.note.is_some_and(|note| note.starts_with("Still working")));
}
