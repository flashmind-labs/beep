use super::*;

#[test]
fn a_machine_not_lent_keeps_its_status_but_gives_no_shell_task_or_screen() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let offers = Offers::local(&[], Provision::new(dir.path()));
    assert!(offers.lends_to_peers(), "lent until the host says not");
    offers.lend_to_peers(false);
    // Offers made earlier from this one share the setting.
    let copy = offers.clone();
    for protocol in [
        "shell/1",
        "terminal/1",
        "device/1",
        "device-video/1",
        "process/1",
        "desktop/1",
        "sign-in/1",
        mcp_servers::PROTOCOL,
        "agents/1",
        "agent-view/1",
        "events/1",
        TRANSCRIPT_PROTOCOL,
        "claude/1",
        "codex/1",
        "hermes-gateway/1",
    ] {
        assert!(
            withheld(protocol, copy.lends_to_peers()).is_some(),
            "{protocol}"
        );
    }
    for protocol in ["status/1", "version/1", "tools/1", "files/1"] {
        assert_eq!(
            withheld(protocol, copy.lends_to_peers()),
            None,
            "{protocol}"
        );
    }

    offers.lend_to_peers(true);
    assert_eq!(withheld("shell/1", copy.lends_to_peers()), None);
}

#[test]
fn known_protocol_names_round_trip_and_compare_as_variants() {
    use beep_proto::node::{
        agent, device, events, mcp_servers, metrics, process, report, shell, sign_in, skill_library, skills,
        status, terminal, tool_update, tools, version,
    };

    for (name, protocol) in [
        (metrics::PROTOCOL, SessionProtocol::Metrics),
        (metrics::QUERY_PROTOCOL, SessionProtocol::MetricsQuery),
        (
            beep_proto::node::logs::QUERY_PROTOCOL,
            SessionProtocol::LogQuery,
        ),
        (report::PROTOCOL, SessionProtocol::Report),
        (status::PROTOCOL, SessionProtocol::Status),
        (version::PROTOCOL, SessionProtocol::Version),
        (tools::PROTOCOL, SessionProtocol::Tools),
        (shell::PROTOCOL, SessionProtocol::Shell),
        (skills::PROTOCOL, SessionProtocol::Skills),
        (skill_library::PROTOCOL, SessionProtocol::SkillLibrary),
        (process::PROTOCOL, SessionProtocol::Tasks),
        (terminal::PROTOCOL, SessionProtocol::Terminal),
        (device::PROTOCOL, SessionProtocol::Device),
        (
            beep_proto::node::device_video::PROTOCOL,
            SessionProtocol::DeviceVideo,
        ),
        (sign_in::PROTOCOL, SessionProtocol::SignIn),
        (mcp_servers::PROTOCOL, SessionProtocol::McpServers),
        (tool_update::CHECK, SessionProtocol::ToolCheck),
        (tool_update::PROTOCOL, SessionProtocol::ToolUpdate),
        (tool_update::FLSH, SessionProtocol::FlshUpdate),
        (events::PROTOCOL, SessionProtocol::Events),
        (agent::SESSION_FOLLOW_PROTOCOL, SessionProtocol::AgentFollow),
        (
            crate::agents::TRANSCRIPT_PROTOCOL,
            SessionProtocol::Transcripts,
        ),
        (agent::SESSIONS_PROTOCOL, SessionProtocol::Agents),
    ] {
        assert_eq!(SessionProtocol::from(name), protocol);
        assert_eq!(protocol.as_ref(), name);
        assert_eq!(protocol.to_string(), name);
    }
}

#[test]
fn unknown_and_host_protocols_keep_their_original_names() {
    for name in ["future/2", "desktop/1", "custom-agent/1"] {
        let protocol = SessionProtocol::from(name);
        assert_eq!(protocol, SessionProtocol::Other(name.to_owned()));
        assert_eq!(protocol.as_ref(), name);
        assert_eq!(protocol.to_string(), name);
    }
}

#[test]
fn another_node_opens_devices_only_while_this_machine_is_lent() {
    use beep_proto::node::{device, shell, status, tools};

    for name in [
        status::PROTOCOL,
        tools::PROTOCOL,
        device::PROTOCOL,
        beep_proto::node::device_video::PROTOCOL,
    ] {
        assert!(node_may_open(&SessionProtocol::from(name)), "{name}");
    }

    assert!(!node_may_open(&SessionProtocol::from(shell::PROTOCOL)));
    assert_eq!(withheld(device::PROTOCOL, true), None);
    assert!(withheld(device::PROTOCOL, false).is_some());
    assert!(withheld(beep_proto::node::device_video::PROTOCOL, false).is_some());
}

#[tokio::test]
async fn only_a_controller_can_read_or_change_mcp_servers() -> Result<()> {
    use beep_proto::node::{local_link, mcp_servers::{Reply, Request}};
    let dir = tempfile::tempdir()?;
    let offers = Offers::local(&[], Provision::new(dir.path()));
    let peer = iroh::SecretKey::generate().public();
    let protocol = SessionProtocol::McpServers;

    let link = local_link();
    let (_, mut rx) = link.caller;
    let (node_tx, node_rx) = link.node;
    serve_open(node_tx, node_rx, protocol.clone(), offers.clone(), Who::Node { peer, desktop_here: false }).await?;
    assert!(matches!(rx.recv::<Opened>().await?, Some(Opened::Refused { .. })));

    let link = local_link();
    let (mut tx, mut rx) = link.caller;
    let (node_tx, node_rx) = link.node;
    let node = tokio::spawn(serve_open(node_tx, node_rx, protocol, offers, Who::Everything(peer)));
    tx.send(&Request::Harnesses).await?;
    assert!(matches!(rx.recv::<Opened>().await?, Some(Opened::Ready {})));
    assert!(matches!(rx.recv::<Reply>().await?, Some(Reply::Harnesses(_))));
    node.await??;

    Ok(())
}

/// A Hermes dashboard that takes one session and one prompt, and yields
/// what the prompt said.
async fn hermes_taking_a_prompt(listener: tokio::net::TcpListener) -> serde_json::Value {
    use futures::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use tokio_tungstenite::tungstenite::Message;

    let (stream, _) = listener.accept().await.expect("the node dials Hermes");
    let mut socket = tokio_tungstenite::accept_async(stream).await.expect("a socket");
    let ready = json!({"jsonrpc": "2.0", "method": "event", "params": {"type": "gateway.ready", "payload": {}}});
    socket.send(Message::text(ready.to_string())).await.expect("sent");
    let results = [
        ("client.capabilities", json!({})),
        ("session.create", json!({"session_id": "live1", "stored_session_id": "stored1"})),
        ("session.events.since", json!({"epoch": 1, "events": [], "latest_seq": 0, "open_requests": []})),
        ("prompt.submit", json!({})),
    ];
    let mut said = Value::Null;
    for (method, result) in results {
        let call = loop {
            if let Some(Ok(Message::Text(text))) = socket.next().await {
                break serde_json::from_str::<Value>(&text).expect("JSON");
            }
        };
        assert_eq!(call["method"], method, "{call}");
        said = call["params"].clone();
        let reply = json!({"jsonrpc": "2.0", "id": call["id"], "result": result});
        socket.send(Message::text(reply.to_string())).await.expect("sent");
    }

    said
}

#[tokio::test]
async fn the_hermes_assistant_opens_through_the_node_and_prompts_hermes() -> Result<()> {
    use crate::agents::HermesLink;
    use beep_proto::node::{
        agent::{Event, Request},
        local_link,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let hermes = tokio::spawn(hermes_taking_a_prompt(listener));
    let dir = tempfile::tempdir()?;
    let offers = Offers::local(&[], Provision::new(dir.path()));
    let token = beep_proto::SecretText::new("t");
    offers.agents.set_hermes(Some(HermesLink { host: "127.0.0.1".into(), port, token }));
    let protocol = SessionProtocol::from("hermes-gateway/1");
    let peer = iroh::SecretKey::generate().public();

    let link = local_link();
    let (mut tx, mut rx) = link.caller;
    let (node_tx, node_rx) = link.node;
    tokio::spawn(serve_open(node_tx, node_rx, protocol, offers, Who::Everything(peer)));
    assert!(matches!(rx.recv::<Opened>().await?, Some(Opened::Ready {})));
    let start = Request::Start {
        cwd: None,
        prompt: "Say hi".into(),
        model: None,
        effort: None,
        checkout: None,
        images: Vec::new(),
        assistant: true,
        fast: None,
    };
    tx.send(&start).await?;

    let attached = tokio::time::timeout(LIMIT, rx.recv::<Event>()).await?;
    assert!(matches!(attached?, Some(Event::Attached { .. })));
    let said = tokio::time::timeout(LIMIT, hermes).await??;
    assert_eq!(said, serde_json::json!({"session_id": "live1", "text": "Say hi"}));

    Ok(())
}

const LIMIT: std::time::Duration = std::time::Duration::from_secs(20);

#[test]
fn an_agent_node_toml_names_outside_the_harnesses_is_offered_but_not_routed() {
    use crate::agents::AgentKind;
    let kind = |name: &str| AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: Vec::new(),
        env: Default::default(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    let dir = tempfile::tempdir().expect("a temp folder");
    let offers = Offers::local(&[kind("claude"), kind("foo")], Provision::new(dir.path()));

    assert!(offers.agents.offers().contains(&"foo/1".to_owned()));
    assert!(route(&SessionProtocol::from("foo/1"), &offers).is_none());
    assert!(matches!(route(&SessionProtocol::from("claude/1"), &offers), Some(Route::Agent(_))));
}
