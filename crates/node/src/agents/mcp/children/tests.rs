use super::*;
use crate::{
    agents::{AgentKind, Begin},
    machine::provision::Provision,
};
use beep_proto::node::agent::{Question, State};
use std::collections::BTreeMap;

fn endpoint() -> EndpointId {
    iroh::SecretKey::generate().public()
}

#[test]
fn a_child_is_started_in_the_same_project_on_the_other_machine() {
    let (me, there) = (endpoint(), endpoint());
    let spec = SyncSpec {
        folders: [
            (me, "/srv/game".to_owned()),
            (there, "/Users/box/game".to_owned()),
        ]
        .into(),
        paused: false,
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
    let view = |info: &SessionInfo| ChildView::of(info, "c1").named("build-box");
    let working = view(&info(Status::Working, 0, None));
    assert_eq!(working.state, "working");
    assert_eq!(working.id, "c1@build-box");
    assert!(!working.settled());
    // Idle before its first turn is still starting.
    assert_eq!(view(&info(Status::Idle, 0, None)).state, "working");
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
fn a_childs_id_names_its_machine() {
    let id = ChildId::parse(" a3f9c1@build-box ");
    assert_eq!((id.session, id.machine), ("a3f9c1", Some("build-box")));
    let id = ChildId::parse("a3f9c1");
    assert_eq!((id.session, id.machine), ("a3f9c1", None));
}

#[test]
fn the_agent_tools_are_the_ones_this_module_answers() {
    for tool in [
        Tool::StartAgent,
        Tool::AgentStatus,
        Tool::SendToAgent,
        Tool::StopAgent,
        Tool::ListAgents,
    ] {
        assert!(handles(tool));
    }
    assert!(!handles(Tool::RunOn));
}

#[test]
fn a_start_call_reads_back_as_a_subagent_card_with_a_link_to_the_child() {
    let view = ChildView {
        id: "a3f9c1@mac-box".into(),
        key: "k1".into(),
        harness: "codex".into(),
        machine: "mac-box".into(),
        state: "working".into(),
        title: None,
        last_message: String::new(),
        files_changed: 0,
        question: None,
        turns: 0,
        tokens: 0,
        cost_usd: None,
        error: None,
        note: None,
    };
    let call = format!(
        "Beep: Start codex agent on mac-box: Build the iOS app [completed]\n{}",
        serde_json::to_string(&view).unwrap()
    );
    assert_eq!(started_view(&call), Some(view.clone()));
    let text = child_entry(&call, &view);
    assert_eq!(
        text,
        "Build the iOS app [in_progress]\n[steps] 0\n[agent] codex on mac-box\n\
         [child] a3f9c1@mac-box working k1"
    );
    let done = ChildView {
        state: "idle".into(),
        turns: 1,
        title: Some("iOS build".into()),
        last_message: "Built and tested.".into(),
        ..view.clone()
    };
    assert_eq!(
        child_entry(&call, &done),
        "iOS build [completed]\n[steps] 1\n[agent] codex on mac-box\n\
         [child] a3f9c1@mac-box idle k1\nBuilt and tested."
    );
    let asking = ChildView {
        state: "waiting_on_user".into(),
        question: Some("Run xcodebuild test".into()),
        ..view
    };
    assert!(child_entry(&call, &asking).ends_with("Waiting for you: Run xcodebuild test"));
    assert_eq!(
        started_view("Beep: Start codex agent: x [completed]\nnot json"),
        None
    );
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
    }
}

async fn tool(offers: &Offers, session: &str, tool: Tool, args: Value) -> (bool, String) {
    let request = Request {
        session: session.to_owned(),
        tool,
        args: args.to_string(),
        approved: Default::default(),
    };
    match call(offers, &request).await {
        Reply::Done { text, failed } => (failed, text),
        other => panic!("not done: {other:?}"),
    }
}

fn view(text: &str) -> ChildView {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_starts_follows_messages_and_stops_its_own_child() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let mut offers = Offers::local(&[], Provision::new(dir.path()), false);
    offers.agents = Arc::new(Agents::new(vec![kind("top"), kind("kid")]));
    let cwd = dir.path().to_string_lossy().into_owned();
    let start = |name: &str| {
        offers
            .agents
            .start(
                &offers.agents.kinds[0],
                Some(&cwd),
                Begin {
                    prompt: Some(format!("hello {name}")),
                    ..Begin::default()
                },
            )
            .expect("a session starts")
    };
    let (mine, theirs) = (start("mine"), start("theirs"));
    for session in [&mine, &theirs] {
        let mut live = session.live.clone();
        tokio::time::timeout(
            Duration::from_secs(20),
            live.wait_for(|live| live.agent_session.is_some() && live.info.turns == 1),
        )
        .await
        .expect("the session settles")
        .expect("the session is kept");
    }
    let (mine, theirs) = (Agents::name_of(&mine), Agents::name_of(&theirs));

    // `wait` returns the answer once the child's turn ends.
    let (failed, text) = tool(
        &offers,
        &mine,
        Tool::StartAgent,
        json!({ "harness": "kid", "prompt": "work", "wait": true }),
    )
    .await;
    assert!(!failed, "{text}");
    let done = view(&text);
    assert_eq!(
        (done.state.as_str(), done.harness.as_str()),
        ("idle", "kid")
    );
    assert_eq!(done.turns, 1);

    // A child that is still working returns at once without `wait`.
    let (failed, text) = tool(
        &offers,
        &mine,
        Tool::StartAgent,
        json!({ "harness": "kid", "prompt": "hang about" }),
    )
    .await;
    assert!(!failed, "{text}");
    let busy = view(&text);
    assert_eq!(busy.state, "working");

    let (_, text) = tool(&offers, &mine, Tool::ListAgents, json!({})).await;
    let listed: Vec<ChildView> = serde_json::from_str(&text).expect("a list");
    assert_eq!(listed.len(), 2, "{text}");

    // Another agent reaches none of them, and learns nothing of them.
    for tool_name in [Tool::AgentStatus, Tool::StopAgent] {
        let (failed, text) = tool(&offers, &theirs, tool_name, json!({ "id": done.id })).await;
        assert!(failed && text.starts_with("You have no agent"), "{text}");
    }
    let (failed, _) = tool(
        &offers,
        &theirs,
        Tool::SendToAgent,
        json!({ "id": done.id, "text": "hi" }),
    )
    .await;
    assert!(failed);
    let (_, text) = tool(&offers, &theirs, Tool::ListAgents, json!({})).await;
    assert_eq!(text, "[]");

    // Its own agent takes a message and reads its answer.
    let (failed, text) = tool(
        &offers,
        &mine,
        Tool::SendToAgent,
        json!({ "id": done.id, "text": "once more" }),
    )
    .await;
    assert!(!failed, "{text}");
    let (_, text) = tool(&offers, &mine, Tool::AgentStatus, json!({ "id": done.id })).await;
    assert!(view(&text).turns >= 1);

    // Stopping one child leaves the other; the parent leaves neither.
    let (failed, _) = tool(&offers, &mine, Tool::StopAgent, json!({ "id": busy.id })).await;
    assert!(!failed);
    let (_, text) = tool(&offers, &mine, Tool::AgentStatus, json!({ "id": busy.id })).await;
    assert_eq!(view(&text).state, "ended");
    let (_, text) = tool(&offers, &mine, Tool::AgentStatus, json!({ "id": done.id })).await;
    assert_ne!(view(&text).state, "ended");
    offers.agents.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_in_a_restricted_mode_starts_no_child_of_another_harness() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let mut offers = Offers::local(&[], Provision::new(dir.path()), false);
    // The harness named `claude` has a full access mode of its own.
    offers.agents = Arc::new(Agents::new(vec![kind("claude"), kind("kid")]));
    let cwd = dir.path().to_string_lossy().into_owned();
    let parent = offers
        .agents
        .start(
            &offers.agents.kinds[0],
            Some(&cwd),
            Begin {
                prompt: Some("hello".into()),
                mode: Some("plan".into()),
                ..Begin::default()
            },
        )
        .expect("a session starts");
    let mut live = parent.live.clone();
    tokio::time::timeout(
        Duration::from_secs(20),
        live.wait_for(|live| live.agent_session.is_some() && live.info.turns == 1),
    )
    .await
    .expect("the session settles")
    .expect("the session is kept");
    let id = Agents::name_of(&parent);
    let (failed, text) = tool(
        &offers,
        &id,
        Tool::StartAgent,
        json!({ "harness": "kid", "prompt": "go" }),
    )
    .await;
    assert!(failed && text.contains("below full access"), "{text}");
    let (failed, text) = tool(
        &offers,
        &id,
        Tool::StartAgent,
        json!({ "harness": "claude", "prompt": "go", "wait": true }),
    )
    .await;
    assert!(!failed, "{text}");
    offers.agents.shutdown().await;
}
