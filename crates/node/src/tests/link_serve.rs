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
    serve_open(node_tx, node_rx, protocol.clone(), offers.clone(), Who::Node(peer)).await?;
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
