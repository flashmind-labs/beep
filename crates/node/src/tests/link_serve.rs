use super::*;
use crate::agents::Parts;

#[test]
fn known_protocol_names_round_trip_and_compare_as_variants() {
    use beep_proto::node::{
        agent, device, events, mcp_servers, metrics, process, report, shell, sign_in,
        skill_library, skills, status, terminal, tool_update, tools, version,
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
    use beep_proto::node::{
        local_link,
        mcp_servers::{Reply, Request},
    };
    let dir = tempfile::tempdir()?;
    let offers = Offers::local(Parts::default(), Provision::new(dir.path()));
    let peer = iroh::SecretKey::generate().public();
    let protocol = SessionProtocol::McpServers;

    let link = local_link();
    let (_, mut rx) = link.caller;
    let (node_tx, node_rx) = link.node;
    serve_open(
        node_tx,
        node_rx,
        protocol.clone(),
        offers.clone(),
        Who::Node {
            peer,
            desktop_here: false,
        },
        false,
    )
    .await?;
    assert!(matches!(
        rx.recv::<Opened>().await?,
        Some(Opened::Refused { .. })
    ));

    let link = local_link();
    let (mut tx, mut rx) = link.caller;
    let (node_tx, node_rx) = link.node;
    let node = tokio::spawn(serve_open(
        node_tx,
        node_rx,
        protocol,
        offers,
        Who::Everything(peer),
        false,
    ));
    tx.send(&Request::Harnesses).await?;
    assert!(matches!(rx.recv::<Opened>().await?, Some(Opened::Ready {})));
    assert!(matches!(
        rx.recv::<Reply>().await?,
        Some(Reply::Harnesses(_))
    ));
    node.await??;

    Ok(())
}

#[test]
fn a_desktop_of_the_mesh_runs_commands_and_agents_only_while_this_machine_is_lent() {
    let offers = Offers::default();
    let own = iroh::SecretKey::generate().public();
    let remote = iroh::SecretKey::generate().public();

    offers.lend_to_peers(false);
    assert!(desktop_commands_off(&offers, remote, own).is_some());
    assert!(desktop_commands_off(&offers, own, own).is_none());

    offers.lend_to_peers(true);
    assert!(desktop_commands_off(&offers, remote, own).is_none());
}

/// What a desktop's `tools/1` RunOn answers, through the real serve path.
async fn desktop_runs(offers: &Offers, who: Who) -> beep_proto::node::tools::Reply {
    use beep_proto::node::{
        local_link,
        tools::{Request, Tool},
    };

    let link = local_link();
    let (mut tx, mut rx) = link.caller;
    let (node_tx, node_rx) = link.node;
    let node = tokio::spawn(serve_open(
        node_tx,
        node_rx,
        SessionProtocol::Tools,
        offers.clone(),
        who,
        false,
    ));
    tx.send(&Request {
        session: beep_proto::SessionId::from("a3f9c1"),
        tool: Tool::RunOn,
        args: r#"{"command":"echo ran-here"}"#.to_owned(),
        approved: Default::default(),
    })
    .await
    .expect("the call is sent");
    assert!(matches!(
        rx.recv::<Opened>().await.expect("opened"),
        Some(Opened::Ready {})
    ));
    let reply = rx.recv().await.expect("a reply").expect("a reply came");
    node.await.expect("joined").expect("served");

    reply
}

#[tokio::test]
async fn a_desktop_of_the_mesh_runs_commands_over_tools_only_while_lent() {
    use beep_proto::node::tools::Reply;
    use iroh::{SecretKey, endpoint::presets};

    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the endpoint binds");
    let dir = tempfile::tempdir().expect("temp dir");
    let offers = Offers::default().bound(&endpoint, dir.path(), "box");
    let remote = SecretKey::generate().public();
    let outcome = |reply| match reply {
        Reply::Done { text, failed } => (text, failed),
        _ => panic!("not done"),
    };

    offers.lend_to_peers(false);
    let (text, failed) = outcome(desktop_runs(&offers, Who::Everything(remote)).await);
    assert!(
        failed && text.contains("Let Beep use this computer is off"),
        "{text}"
    );
    assert!(!text.contains("ran-here"));
    let (text, failed) = outcome(desktop_runs(&offers, Who::Everything(endpoint.id())).await);
    assert!(!failed && text.contains("ran-here"), "{text}");

    offers.lend_to_peers(true);
    let (text, failed) = outcome(desktop_runs(&offers, Who::Everything(remote)).await);
    assert!(!failed && text.contains("ran-here"), "{text}");
}
