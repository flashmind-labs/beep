use super::*;
use crate::mesh::Retired;
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
        let _ = rx.recv::<u8>().await;
        Ok(())
    }
}
