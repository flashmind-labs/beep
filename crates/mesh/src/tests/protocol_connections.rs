use super::*;
use crate::inner::Retired;
use crate::{DeviceKind, JOIN_ALPN, PEER_ALPN, PeerClient};
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use std::time::Duration;

#[tokio::test]
async fn protocol_connections_prefer_the_live_mesh_path_to_a_stale_saved_address() {
    let pair = pair().await;
    let controller = &pair.controller;
    let node = &pair.node;
    let id = node.me();
    let wrong_endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    controller.inner.addrs().get_mut(&id).unwrap().addr =
        EndpointAddr::new(id).with_addrs(wrong_endpoint.addr().addrs);
    let client = PeerClient::over(controller.clone(), id);
    let _stream = timeout(Duration::from_secs(2), client.open_commands())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn a_new_mesh_session_closes_incoming_and_outgoing_protocol_connections() {
    let pair = pair().await;
    let controller = &pair.controller;
    let node = &pair.node;
    let client = PeerClient::over(controller.clone(), node.me());
    let _stream = client.open_commands().await.unwrap();
    let connection = controller.command_connection(node.me()).await.unwrap();
    let peer_session = node.inner.conns().get(&controller.me()).unwrap().session;
    node.inner
        .restart_connections(controller.me(), peer_session);
    assert!(connection.close_reason().is_none());

    node.inner
        .restart_connections(controller.me(), peer_session.wrapping_add(1));
    timeout(Duration::from_secs(2), connection.closed())
        .await
        .unwrap();

    let client = PeerClient::over(controller.clone(), node.me());
    let _stream = client.open_commands().await.unwrap();
    let connection = controller.command_connection(node.me()).await.unwrap();
    let peer_session = controller.inner.conns().get(&node.me()).unwrap().session;
    controller
        .inner
        .restart_connections(node.me(), peer_session.wrapping_add(1));
    assert!(connection.close_reason().is_some());
}

#[tokio::test]
async fn a_mesh_hello_closes_protocol_connections_without_a_known_session() {
    let pair = pair().await;
    let client = PeerClient::over(pair.controller.clone(), pair.node.me());
    let _stream = client.open_commands().await.unwrap();
    let connection = pair
        .controller
        .command_connection(pair.node.me())
        .await
        .unwrap();
    let peer = pair.controller.me();
    let session = pair.node.inner.conns().get(&peer).unwrap().session;
    {
        let channels = pair.node.inner.channels.lock().unwrap();
        let slot = channels
            .get(&(peer, beep_proto::node::commands::ALPN))
            .unwrap();
        let mut incoming = slot.incoming.lock().unwrap();
        assert_eq!(incoming.len(), 1);
        incoming[0].peer_session = None;
    }

    pair.node.inner.restart_connections(peer, session);
    timeout(Duration::from_secs(2), connection.closed())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_new_mesh_session_closes_retired_links_even_without_a_current_link() {
    let pair = pair().await;
    let peer = pair.node.me();
    let connection = pair.controller.peer(peer).unwrap();
    let session = pair.controller.inner.conns().get(&peer).unwrap().session;
    pair.controller.inner.retired().insert(
        connection.stable_id(),
        Retired {
            conn: connection.clone(),
            session,
        },
    );
    pair.controller.inner.conns().remove(&peer);
    pair.controller.inner.restart_connections(peer, session);
    assert!(connection.close_reason().is_none());

    pair.controller
        .inner
        .restart_connections(peer, session.wrapping_add(1));
    assert!(connection.close_reason().is_some());
}

#[tokio::test]
async fn rejected_protocol_dials_share_a_cooldown_and_retry_after_restart() {
    let pair = pair().await;
    let mesh = &pair.controller;
    let peer = pair.node.me();
    // This peer accepts commands but has no subscription handler.
    assert!(mesh.subscription_connection(peer).await.is_err());
    let slot = mesh
        .inner
        .channels
        .lock()
        .unwrap()
        .get(&(peer, beep_proto::node::subscriptions::ALPN))
        .unwrap()
        .clone();
    let retry_at = slot.dialing.lock().await.as_ref().unwrap().retry_at;
    let calls = (0..16).map(|_| mesh.subscription_connection(peer));
    for result in futures::future::join_all(calls).await {
        assert!(result.is_err());
    }
    assert_eq!(
        slot.dialing.lock().await.as_ref().unwrap().retry_at,
        retry_at
    );
    // A failure on subscriptions must not block commands.
    assert!(mesh.command_connection(peer).await.is_ok());

    let session = {
        let mut conns = mesh.inner.conns();
        let live = conns.get_mut(&peer).unwrap();
        live.session = live.session.wrapping_add(1);
        live.session
    };
    mesh.inner.restart_connections(peer, session);
    assert!(mesh.subscription_connection(peer).await.is_err());
    let failed = slot.dialing.lock().await;
    assert_eq!(failed.as_ref().unwrap().peer_session, Some(session));
}

#[tokio::test]
async fn a_rejected_protocol_dial_is_tried_again_after_retry() {
    let pair = pair().await;
    let mesh = &pair.controller;
    let peer = pair.node.me();
    assert!(mesh.subscription_connection(peer).await.is_err());
    let slot = mesh
        .inner
        .channels
        .lock()
        .unwrap()
        .get(&(peer, beep_proto::node::subscriptions::ALPN))
        .unwrap()
        .clone();
    {
        let mut failed = slot.dialing.lock().await;
        // Expire the first failure without waiting on wall clock time.
        failed.as_mut().unwrap().retry_at = Instant::now();
    }
    let tried = Instant::now();
    assert!(mesh.subscription_connection(peer).await.is_err());
    let retry_at = slot.dialing.lock().await.as_ref().unwrap().retry_at;
    assert!(retry_at >= tried + RETRY);
    assert!(retry_at <= Instant::now() + RETRY);
}

#[tokio::test]
async fn a_protocol_connection_that_takes_no_new_stream_is_dialed_again() {
    let pair = pair().await;
    let peer = pair.node.me();
    let client = PeerClient::over(pair.controller.clone(), peer);
    let _first = client.open_commands().await.unwrap();
    let full = pair.controller.command_connection(peer).await.unwrap();

    let Err(refused) = client.open_commands().await else {
        panic!("a full connection took a stream");
    };
    assert!(refused.is_transport(), "{refused:#}");
    timeout(Duration::from_secs(2), full.closed())
        .await
        .unwrap();

    // The next stream goes on a new connection, over the same mesh link.
    let _again = timeout(Duration::from_secs(5), client.open_commands())
        .await
        .unwrap()
        .unwrap();
    let fresh = pair.controller.command_connection(peer).await.unwrap();
    assert_ne!(fresh.stable_id(), full.stable_id());
    assert!(pair.controller.peer(peer).is_some());
}

#[tokio::test]
async fn reconnecting_dials_the_mesh_link_and_protocols_again() {
    let pair = pair().await;
    let peer = pair.node.me();
    let client = PeerClient::over(pair.controller.clone(), peer);
    let _stream = client.open_commands().await.unwrap();
    let link = pair.controller.peer(peer).unwrap();
    let commands = pair.controller.command_connection(peer).await.unwrap();

    pair.controller.reconnect().await;
    timeout(Duration::from_secs(2), commands.closed())
        .await
        .unwrap();
    let mut changed = pair.controller.updates();
    let relinked = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(live) = pair.controller.peer(peer)
                && live.stable_id() != link.stable_id()
            {
                return live;
            }

            changed.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(relinked.close_reason().is_none());

    let _again = timeout(Duration::from_secs(5), client.open_commands())
        .await
        .unwrap()
        .unwrap();
    let fresh = pair.controller.command_connection(peer).await.unwrap();
    assert_ne!(fresh.stable_id(), commands.stable_id());
}

struct Pair {
    controller: Mesh,
    node: Mesh,
    _routers: [Router; 2],
    _dirs: [tempfile::TempDir; 2],
}

async fn pair() -> Pair {
    let controller_endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let controller_dir = tempfile::tempdir().unwrap();
    let controller = Mesh::create(
        controller_dir.path(),
        controller_endpoint.clone(),
        "controller",
    )
    .await
    .unwrap();
    let (peer, join) = controller.protocols();
    let controller_router = Router::builder(controller_endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn();
    let ticket = controller
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .unwrap();
    let node_endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let node_dir = tempfile::tempdir().unwrap();
    let node = Mesh::join(node_dir.path(), node_endpoint.clone(), &ticket, "node")
        .await
        .unwrap();
    let (peer, join) = node.protocols();
    let node_router = Router::builder(node_endpoint.clone())
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .accept(beep_proto::node::commands::ALPN, Commands(node.clone()))
        .spawn();
    let id = node_endpoint.id();
    let mut changed = controller.updates();
    timeout(Duration::from_secs(5), async {
        while controller.peer(id).is_none() {
            changed.changed().await.unwrap();
        }
    })
    .await
    .unwrap();

    Pair {
        controller,
        node,
        _routers: [controller_router, node_router],
        _dirs: [controller_dir, node_dir],
    }
}

#[derive(Debug)]
struct Commands(Mesh);

impl iroh::protocol::ProtocolHandler for Commands {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        use beep_proto::node::{commands, link};
        self.0.track_protocol_connection(&connection).unwrap();
        let (send, recv) = connection.accept_bi().await.unwrap();
        let (mut tx, mut rx) = link(send, recv);
        assert_eq!(
            rx.recv::<commands::Open>().await.unwrap(),
            Some(commands::Open::Calls)
        );
        tx.send(&commands::Reply::Ready).await.unwrap();
        // Full from here on: further streams are reset at once, as a node
        // does past its stream cap, while the connection stays open.
        let more = connection.clone();
        tokio::spawn(async move {
            while let Ok((mut send, mut recv)) = more.accept_bi().await {
                send.reset(1u32.into()).ok();
                recv.stop(1u32.into()).ok();
            }
        });
        let _ = rx.recv::<u8>().await;
        Ok(())
    }
}
