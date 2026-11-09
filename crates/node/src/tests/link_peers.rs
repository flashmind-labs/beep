use super::*;
use iroh::EndpointId;
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
        status_with(tx, rx, Offers::default(), std::future::pending).await
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
    let peers = Peers::new(
        node.clone(),
        node_dir.path().to_owned(),
        "box".to_owned(),
        Offers::default(),
    );
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
    let offers = Offers::default();
    let peers = Peers::new(
        endpoint.clone(),
        dir.path().to_owned(),
        "box".to_owned(),
        offers.clone(),
    );
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
    let peers = Peers::new(
        node.clone(),
        dir.path().to_owned(),
        "box".to_owned(),
        offers,
    );
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
            peers.inner.serve(tx, rx, who).await.expect("served");
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

/// Another node of the mesh is refused a machine's report.
#[tokio::test]
async fn a_node_is_refused_a_report() {
    let (answer, reply) = ask_report(|peer| Who::Node {
        peer,
        desktop_here: false,
    })
    .await;
    assert!(matches!(answer, Some(Opened::Refused { .. })), "{answer:?}");
    assert!(reply.is_none(), "{reply:?}");
}

/// Opens `report/1` on a node that is not lent, from a caller `who`
/// makes of its key, and asks for the share when the node is ready.
async fn ask_report(who: fn(EndpointId) -> Who) -> (Option<Opened>, Option<report::Reply>) {
    ask_report_using(who, report::PROTOCOL).await
}

#[tokio::test]
async fn a_node_is_refused_another_machines_metrics() {
    let (answer, reply) = ask_report_using(
        |peer| Who::Node {
            peer,
            desktop_here: false,
        },
        beep_proto::node::metrics::PROTOCOL,
    )
    .await;
    assert!(matches!(answer, Some(Opened::Refused { .. })), "{answer:?}");
    assert!(reply.is_none());
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
    let peers = Peers::new(
        node.clone(),
        dir.path().to_owned(),
        "box".to_owned(),
        offers,
    );
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
        peers.inner.serve(tx, rx, who).await.expect("served");
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

    if matches!(answer, Some(Opened::Ready {})) {
        tx.send(&report::Request {
            since: 0,
            max_bytes: 1024,
        })
        .await
        .expect("the request is sent");
        reply = timeout(Duration::from_secs(30), rx.recv::<report::Reply>())
            .await
            .expect("a reply came")
            .expect("it reads");
    }

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
