use super::*;
use beep_mesh::PeerClient;
use beep_proto::node::{Opened, report};
use iroh::{Endpoint, EndpointId};
use iroh_tickets::Ticket;

/// Answers the wait's probe, then one join, the way the node does.
fn serve_probe_then_join(mut listener: Listener) {
    tokio::spawn(async move {
        for _ in 0..2 {
            let stream = listener.accept().await.expect("accepting");
            let (read, mut write) = split(stream);
            let request = BufReader::new(read)
                .lines()
                .next_line()
                .await
                .expect("reading");
            if request.is_some_and(|line| line.starts_with(JOIN)) {
                write.write_all(b"joined\n").await.expect("writing");
            }
        }
    });
}

#[tokio::test]
async fn status_reports_and_ends_while_the_probe_is_still_running() {
    use iroh::{SecretKey, endpoint::presets};
    const ALPN: &[u8] = b"status-test";
    let bind = || async {
        Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .expect("the endpoint binds")
    };
    let (node, desktop) = (bind().await, bind().await);
    let addr = node.addr();
    let dialing = tokio::spawn({
        let desktop = desktop.clone();
        async move { desktop.connect(addr, ALPN).await }
    });
    let incoming = node.accept().await.expect("a connection comes");
    let from_desktop = incoming.await.expect("it is accepted");
    let to_node = dialing.await.expect("joined").expect("connected");

    // The node serves status with a probe that never finishes.
    let serving = tokio::spawn(async move {
        let (send, recv) = from_desktop.open_bi().await.expect("a stream opens");
        let (tx, rx) = link(send, recv);
        let probes = env::Probes::new(|| Box::pin(std::future::pending()));
        status_with(tx, rx, Offers::default(), &probes).await
    });
    let (send, recv) = to_node.accept_bi().await.expect("the stream arrives");
    let (client_tx, mut reports) = link(send, recv);
    let (mut stats, mut traffic) = (false, false);
    timeout(Duration::from_secs(30), async {
        while !(stats && traffic) {
            match reports.recv::<Report>().await.expect("a report reads") {
                Some(Report::Stats(_)) => stats = true,
                Some(Report::Traffic(_)) => traffic = true,
                Some(Report::Env(_)) => panic!("the probe never finished"),
                Some(_) => {}
                None => panic!("the stream ended"),
            }
        }
    })
    .await
    .expect("the reports came while the probe ran");
    // The desktop leaving ends the stream without waiting for the probe.
    drop(client_tx);
    to_node.close(0u32.into(), b"done");
    timeout(Duration::from_secs(30), serving)
        .await
        .expect("status ended with the peer")
        .expect("the task joined")
        .ok();
}

/// A peer's connection in its handshake must not keep a leave from
/// closing the mesh.
#[tokio::test]
async fn leaving_does_not_wait_for_a_connection_still_in_its_handshake() {
    use beep_mesh::{JOIN_ALPN, PEER_ALPN};
    use iroh::{SecretKey, protocol::Router};
    let bind = || async {
        Endpoint::builder(iroh::endpoint::presets::Minimal)
            .secret_key(SecretKey::generate())
            .bind()
            .await
            .expect("the endpoint binds")
    };
    let (desktop, node) = (bind().await, bind().await);
    let desktop_dir = tempfile::tempdir().expect("temp dir");
    let mesh = Mesh::create(desktop_dir.path(), desktop.clone(), "desktop")
        .await
        .expect("the mesh is created");
    let (peer, join) = mesh.protocols();
    let _desktop_router = Router::builder(desktop.clone())
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn();
    let ticket = mesh
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .expect("a ticket");
    let node_dir = tempfile::tempdir().expect("temp dir");
    let peers = Peers::new(Offers::default().bound(&node, node_dir.path(), "box")).await;
    let _node_router = Router::builder(node.clone())
        .accept(PEER_ALPN, peers.peer_handler())
        .accept(JOIN_ALPN, peers.join_handler())
        .spawn();
    peers
        .join(&ticket.encode_string())
        .await
        .expect("the node joins");
    // The desktop opens the sync stream and sends half a frame: the node
    // answers with its hello and then waits for the rest.
    let conn = desktop
        .connect(node.addr(), PEER_ALPN)
        .await
        .expect("connected");
    let (mut send, mut recv) = conn.open_bi().await.expect("a stream opens");
    send.write_all(&[0]).await.expect("a byte is sent");
    let mut hello = [0u8; 1];
    timeout(Duration::from_secs(30), recv.read_exact(&mut hello))
        .await
        .expect("the node answered")
        .expect("the hello reads");
    timeout(CLOSE_WAIT / 2, peers.forget())
        .await
        .expect("the leave did not wait for the connection")
        .expect("the node leaves");
    drop((send, recv, conn));
}

#[tokio::test]
async fn transfers_served_to_the_mesh_reach_the_offers_it_was_made_from() {
    use crate::files::{Folders, syncthing::Syncthing};
    use iroh::{SecretKey, endpoint::presets};
    let dir = tempfile::tempdir().expect("temp dir");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the endpoint binds");
    let offers = Offers::default().bound(&endpoint, dir.path(), "box");
    let peers = Peers::new(offers.clone()).await;
    let transfers = Transfers::start_with(
        &endpoint,
        &dir.path().join("blobs"),
        Folders::new(&[]),
        Syncthing::at(vec![dir.path().join("syncthing")], None),
    )
    .await
    .expect("the transfers start");
    peers.serve_transfers(transfers.clone());
    // The host's own copy, which its agents' tools read, sees them too.
    assert!(offers.transfers.get().is_some());
    transfers.shutdown().await.expect("the transfers stop");
}

/// A device of the mesh opening an agent stream on a machine that is not
/// lent is refused, over a real connection.
#[tokio::test]
async fn a_machine_not_lent_refuses_agent_streams_from_other_desktops() {
    use beep_proto::node::Open;
    use iroh::{SecretKey, endpoint::presets};
    const ALPN: &[u8] = b"withheld-test";
    let bind = || async {
        Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .expect("the endpoint binds")
    };
    let (node, desktop) = (bind().await, bind().await);
    let dir = tempfile::tempdir().expect("temp dir");
    let offers = Offers::default();
    offers.lend_to_peers(false);
    let peers = Peers::new(offers.bound(&node, dir.path(), "box")).await;
    let addr = node.addr();
    let dialing = tokio::spawn({
        let desktop = desktop.clone();
        async move { desktop.connect(addr, ALPN).await }
    });
    let from_desktop = node
        .accept()
        .await
        .expect("a connection comes")
        .await
        .expect("it is accepted");
    let to_node = dialing.await.expect("joined").expect("connected");
    let who = Who::Everything(desktop.id());
    let serving = tokio::spawn(async move {
        for _ in 0..2 {
            let (send, recv) = from_desktop.accept_bi().await.expect("a stream arrives");
            let (tx, rx) = link(send, recv);
            peers.inner.clone().serve(tx, rx, who).await.expect("served");
        }
        // Dropping the connection now would cut off the last answer.
        from_desktop
    });

    for protocol in ["agents/1", "claude/1"] {
        let (send, recv) = to_node.open_bi().await.expect("a stream opens");
        let (mut tx, mut rx) = link(send, recv);
        tx.send(&Open {
            protocol: protocol.to_owned(),
        })
        .await
        .expect("the protocol is sent");
        let answer = timeout(Duration::from_secs(30), rx.recv::<Opened>())
            .await
            .expect("an answer came")
            .expect("it reads");
        assert!(
            matches!(answer, Some(Opened::Refused { .. })),
            "{protocol}: {answer:?}"
        );
    }

    drop(serving.await.expect("served both"));
}

/// `report/1` stays open to a desktop on a machine that is not lent.
#[tokio::test]
async fn a_machine_not_lent_still_gives_a_desktop_its_report() {
    let (answer, reply) = ask_report(Who::Everything).await;
    assert!(matches!(answer, Some(Opened::Ready {})), "{answer:?}");
    assert!(
        matches!(&reply, Some(report::Reply::Share { log, .. }) if log == "a line\n"),
        "{reply:?}"
    );
}

/// Another node of the mesh gets a machine's report, as a desktop does.
#[tokio::test]
async fn a_node_gets_a_report() {
    let (answer, reply) = ask_report(another_node).await;
    assert!(matches!(answer, Some(Opened::Ready {})), "{answer:?}");
    assert!(
        matches!(&reply, Some(report::Reply::Share { log, .. }) if log == "a line\n"),
        "{reply:?}"
    );
}

/// Another node of the mesh may open a machine's log and metrics.
#[tokio::test]
async fn a_node_opens_another_machines_log_and_metrics() {
    for protocol in [
        beep_proto::node::metrics::PROTOCOL,
        beep_proto::node::metrics::QUERY_PROTOCOL,
        beep_proto::node::logs::QUERY_PROTOCOL,
    ] {
        let (answer, _) = ask_report_using(another_node, protocol).await;
        assert!(
            matches!(answer, Some(Opened::Ready {})),
            "{protocol}: {answer:?}"
        );
    }
}

fn another_node(peer: EndpointId) -> Who {
    Who::Node {
        peer,
        desktop_here: false,
    }
}

/// Opens `report/1` on a node that is not lent, from a caller `who`
/// makes of its key, and asks for the share when the node is ready.
async fn ask_report(who: fn(EndpointId) -> Who) -> (Option<Opened>, Option<report::Reply>) {
    ask_report_using(who, report::PROTOCOL).await
}

async fn ask_report_using(
    who: fn(EndpointId) -> Who,
    protocol: &str,
) -> (Option<Opened>, Option<report::Reply>) {
    use crate::machine::service::logs::Logs;
    use beep_proto::node::Open;
    use iroh::{SecretKey, endpoint::presets};
    const ALPN: &[u8] = b"report-test";
    let bind = || async {
        Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .expect("the endpoint binds")
    };
    let (node, desktop) = (bind().await, bind().await);
    let dir = tempfile::tempdir().expect("temp dir");
    let log = dir.path().join("beep.log");
    std::fs::write(&log, "a line\n").expect("the temp folder is writable");
    let offers = Offers {
        log: Logs::from(log),
        ..Offers::default()
    };
    offers.lend_to_peers(false);
    let peers = Peers::new(offers.bound(&node, dir.path(), "box")).await;
    let addr = node.addr();
    let dialing = tokio::spawn({
        let desktop = desktop.clone();
        async move { desktop.connect(addr, ALPN).await }
    });
    let from_desktop = node
        .accept()
        .await
        .expect("a connection comes")
        .await
        .expect("it is accepted");
    let to_node = dialing.await.expect("joined").expect("connected");
    let who = who(desktop.id());
    let serving = tokio::spawn(async move {
        let (send, recv) = from_desktop.accept_bi().await.expect("a stream arrives");
        let (tx, rx) = link(send, recv);
        peers.inner.clone().serve(tx, rx, who).await.expect("served");
        from_desktop
    });
    let (send, recv) = to_node.open_bi().await.expect("a stream opens");
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Open {
        protocol: protocol.to_owned(),
    })
    .await
    .expect("the protocol is sent");
    let answer = timeout(Duration::from_secs(30), rx.recv::<Opened>())
        .await
        .expect("an answer came")
        .expect("it reads");
    let mut reply = None;

    if matches!(answer, Some(Opened::Ready {})) && protocol == report::PROTOCOL {
        tx.send(&report::Request {
            since: 0,
            max_bytes: 1024,
            whole: false,
        })
        .await
        .expect("the request is sent");
        reply = timeout(Duration::from_secs(30), rx.recv::<report::Reply>())
            .await
            .expect("a reply came")
            .expect("it reads");
    }

    drop(tx);
    drop(rx);
    drop(serving.await.expect("served"));
    (answer, reply)
}

#[tokio::test]
async fn join_waits_for_a_node_that_starts_late() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut retries = 0;
    // The socket opens after the first attempt failed, as it does for a
    // node that was just started.
    wait_for_node(dir.path(), Duration::from_secs(10), || {
        retries += 1;

        if retries == 1 {
            let listener = Listener::bind(&dir.path().join(SOCKET)).expect("bind");
            serve_probe_then_join(listener);
        }
    })
    .await
    .expect("node came up");
    assert!(retries >= 1, "the first attempt found no node");
    join(dir.path(), "ticket").await.expect("joined");
}

#[tokio::test]
async fn join_says_what_to_do_when_no_node_comes_up() {
    let dir = tempfile::tempdir().expect("temp dir");
    let err = join_when_up(dir.path(), "ticket", Duration::from_millis(300))
        .await
        .expect_err("no node");
    assert!(format!("{err}").contains("beep-node run"), "{err}");
}

struct ChannelPair {
    desktop: Mesh,
    peers: Peers,
    endpoint: Endpoint,
    _routers: (iroh::protocol::Router, iroh::protocol::Router),
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn channel_pair(dedicated: bool) -> ChannelPair {
    use beep_mesh::{JOIN_ALPN, PEER_ALPN};
    use beep_proto::node::{commands, forward, subscriptions};
    use iroh::{SecretKey, protocol::Router};
    let bind = || async {
        Endpoint::builder(iroh::endpoint::presets::Minimal)
            .secret_key(SecretKey::generate())
            .transport_config(beep_mesh::transport_config())
            .bind()
            .await
            .unwrap()
    };
    let (desktop_endpoint, endpoint) = (bind().await, bind().await);
    let dirs = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let desktop = Mesh::create(dirs.0.path(), desktop_endpoint.clone(), "desktop")
        .await
        .unwrap();
    let (peer, join) = desktop.protocols();
    let desktop_router = Router::builder(desktop_endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn();
    let ticket = desktop
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .unwrap();
    let peers = Peers::new(Offers::default().bound(&endpoint, dirs.1.path(), "node")).await;
    let mut router = Router::builder(endpoint.clone())
        .accept(PEER_ALPN, peers.peer_handler())
        .accept(JOIN_ALPN, peers.join_handler());
    if dedicated {
        router = router
            .accept(commands::ALPN, crate::link::commands::CommandsAlpn::new(&peers))
            .accept(subscriptions::ALPN, crate::link::subscriptions::SubscriptionsAlpn::new(&peers))
            .accept(forward::ALPN, crate::link::forward::ForwardAlpn::new(&peers));
    }
    let node_router = router.spawn();
    peers.join(&ticket.encode_string()).await.unwrap();
    let mut changes = desktop.updates();
    timeout(Duration::from_secs(30), async {
        while desktop.peer(endpoint.id()).is_none() {
            changes.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    ChannelPair {
        desktop,
        peers,
        endpoint,
        _routers: (desktop_router, node_router),
        _dirs: dirs,
    }
}

#[tokio::test]
async fn dedicated_peer_protocols_reuse_connections_and_close_when_membership_ends() {
    use beep_proto::node::{
        commands,
        events::{self, CallReply, Operation, Topic, Value},
        process, subscriptions,
    };
    let pair = channel_pair(true).await;
    let peer = pair.endpoint.id();
    let client = PeerClient::over(pair.desktop.clone(), peer);
    let (mut command_tx, mut command_rx) = client.open_commands().await.unwrap();
    let command = pair.desktop.command_connection(peer).await.unwrap();
    assert_eq!(command.alpn(), commands::ALPN);

    let topic = Topic::keyed("plugin", "example");
    pair.peers
        .channel_offers()
        .agents
        .events
        .publish(topic.clone(), vec![1])
        .unwrap();
    let (mut tx, mut rx) = client.open_subscriptions().await.unwrap();
    tx.send(&subscriptions::Request::Watch {
        id: 1,
        topic: topic.clone(),
        queries: vec![],
        deltas: false,
    })
    .await
    .unwrap();
    assert_eq!(
        rx.recv::<subscriptions::Reply>().await.unwrap(),
        Some(subscriptions::Reply::Watching { id: 1 })
    );
    assert!(
        matches!(rx.recv::<subscriptions::Reply>().await.unwrap(), Some(subscriptions::Reply::Snapshot(value)) if value.topic == topic && value.value == Value::Plugin(vec![1]))
    );
    let connection = pair.desktop.subscription_connection(peer).await.unwrap();
    assert_ne!(connection.stable_id(), command.stable_id());
    assert_eq!(connection.alpn(), subscriptions::ALPN);
    let (mut extra_tx, mut extra_rx) = client.open_subscriptions().await.unwrap();
    extra_tx
        .send(&subscriptions::Request::Ping { id: 7 })
        .await
        .unwrap();
    assert_eq!(
        extra_rx.recv::<subscriptions::Reply>().await.unwrap(),
        Some(subscriptions::Reply::Pong { id: 7 })
    );
    assert_eq!(
        connection.stable_id(),
        pair.desktop
            .subscription_connection(peer)
            .await
            .unwrap()
            .stable_id()
    );

    command_tx
        .send(&commands::Request::Call {
            id: 2,
            operation: Box::new(Operation::Process(process::Request::List {})),
        })
        .await
        .unwrap();
    assert!(
        matches!(command_rx.recv::<commands::Reply>().await.unwrap(), Some(commands::Reply::Result { id: 2, reply }) if matches!(*reply, CallReply::Process(_)))
    );

    let sessions: beep_proto::node::agent::SessionsReply = client
        .call(
            beep_proto::node::agent::SESSIONS_PROTOCOL,
            &beep_proto::node::agent::SessionsRequest::ListAll {},
        )
        .await
        .unwrap();
    assert!(matches!(
        sessions,
        beep_proto::node::agent::SessionsReply::Sessions { .. }
    ));
    assert_eq!(
        command.stable_id(),
        pair.desktop
            .command_connection(peer)
            .await
            .unwrap()
            .stable_id()
    );

    // Released phones keep their existing inbound events/1 protocol.
    let released = PeerClient::new(pair.desktop.peer(peer).unwrap());
    let (_tx, mut rx) = released
        .open_sending(
            events::PROTOCOL,
            &events::Request::Watch {
                id: 1,
                topics: vec![Topic::new("agents")],
                queries: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        rx.recv::<events::Reply>().await.unwrap(),
        Some(events::Reply::Ready)
    );

    let stranger = Endpoint::builder(iroh::endpoint::presets::Minimal)
        .bind()
        .await
        .unwrap();
    for alpn in [commands::ALPN, subscriptions::ALPN] {
        let connection = stranger.connect(pair.endpoint.addr(), alpn).await.unwrap();
        timeout(Duration::from_secs(5), connection.closed())
            .await
            .unwrap();
    }

    timeout(Duration::from_secs(10), pair.peers.leave())
        .await
        .unwrap();
    timeout(Duration::from_secs(5), command.closed())
        .await
        .unwrap();
    timeout(Duration::from_secs(5), connection.closed())
        .await
        .unwrap();
}

#[tokio::test]
async fn peers_without_the_new_protocols_are_rejected_without_fallback() {
    let pair = channel_pair(false).await;
    let client = PeerClient::over(pair.desktop.clone(), pair.endpoint.id());
    assert!(client.open_commands().await.is_err());
    assert!(client.open_subscriptions().await.is_err());
    assert!(
        pair.desktop
            .forward_connection(pair.endpoint.id())
            .await
            .is_err()
    );
}

/// A node that left its mesh serves none of its teams any more.
#[tokio::test]
async fn leaving_the_mesh_forgets_the_teams() {
    use beep_proto::team::{Member, TeamRecord, TeamRepo};
    use iroh::{EndpointAddr, SecretKey, endpoint::presets};
    let dir = tempfile::tempdir().expect("temp dir");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the endpoint binds");
    let peers = Peers::new(Offers::default().bound(&endpoint, dir.path(), "box")).await;
    let repo: TeamRepo = "github.com/org/repo".parse().expect("a repo");
    let alice = Member {
        login: "alice".parse().expect("a login"),
        mesh: SecretKey::generate().public(),
        devices: vec![EndpointAddr::new(endpoint.id())],
        joined_at: std::time::SystemTime::now(),
        admitted_by: None,
        proof: None,
    };
    peers.inner.team.with(|teams| {
        teams
            .rosters
            .refresh([(repo.clone(), TeamRecord::new(repo.clone(), vec![alice]))].into());
    });
    peers.inner.team.publish(|served| {
        served.boards.set(&repo, Vec::new());
        served.gateways.insert(repo.clone(), endpoint.id());
    });

    peers.leave().await;

    peers.inner.team.with(|teams| assert!(teams.rosters.get(&repo).is_none()));
    assert!(!peers.inner.team.served(|served| served.boards.serves(&repo)));
    assert!(peers.inner.team.served(|served| served.gateways.is_empty()));
}
