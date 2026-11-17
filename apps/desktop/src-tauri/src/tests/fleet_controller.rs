use super::*;
use beep_mesh::{Key, transport_config};
use iroh::{SecretKey, endpoint::presets};
use tempfile::TempDir;

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the local endpoint binds")
}

#[tokio::test]
async fn a_controller_keeps_desktop_authority_without_a_local_host() {
    let dir = TempDir::new().expect("a temporary directory");
    let endpoint = endpoint().await;
    let fleet = Fleet::controller(endpoint.clone(), dir.path(), "build-box");
    fleet.run().await;
    let mesh = fleet.ready().await.expect("the controller makes its mesh");
    let this = mesh
        .devices()
        .into_iter()
        .find(|device| device.device.id == fleet.id())
        .expect("the controller is a member");

    assert_eq!(this.device.kind, DeviceKind::Desktop);
    assert!(mesh.is_observer(fleet.id()));
    assert!(mesh.is_desktop_observer(fleet.id()));
    assert!(mesh.code_hosts().is_empty());
    assert!(
        fleet.online().is_empty(),
        "a controller offers no local host"
    );
    assert!(matches!(
        fleet.client(fleet.id()).await,
        Err(DesktopError::BadRequest(_))
    ));
    let ticket = fleet
        .invite(DeviceKind::Node)
        .await
        .expect("a desktop can invite nodes");
    assert_eq!(ticket.kind, DeviceKind::Node);
    mesh.set(
        Key::Other("example/setting".into()),
        Some(b"saved".to_vec()),
    )
    .await
    .expect("the controller can write shared settings");
    fleet.stop().await;
    drop(mesh);
    drop(fleet);

    let reopened = Fleet::controller(endpoint.clone(), dir.path(), "build-box");
    reopened.run().await;
    let mesh = reopened.ready().await.expect("the saved mesh opens");
    assert_eq!(
        mesh.get(&Key::Other("example/setting".into())),
        Some(b"saved".to_vec())
    );
    assert_eq!(mesh.me(), endpoint.id());
    reopened.stop().await;
    endpoint.close().await;
}

#[tokio::test]
async fn a_remote_member_can_redeem_a_controllers_ticket() {
    let dir = TempDir::new().expect("a temporary directory");
    let remote_dir = TempDir::new().expect("a temporary directory");
    let endpoint = endpoint().await;
    let fleet = Fleet::controller(endpoint.clone(), dir.path(), "build-box");
    fleet.run().await;
    let ticket = fleet.invite(DeviceKind::Node).await.expect("a ticket");
    let remote = self::endpoint().await;
    let joined = timeout(
        Duration::from_secs(20),
        Mesh::join(remote_dir.path(), remote.clone(), &ticket, "gpu-box"),
    )
    .await
    .expect("the join answers in time")
    .expect("the node joins through mesh protocols");

    assert!(joined.is_member(fleet.id()));
    assert!(
        fleet
            .ready()
            .await
            .expect("the mesh stays open")
            .is_member(remote.id())
    );
    fleet.stop().await;
    drop(joined);
    remote.close().await;
    endpoint.close().await;
}

#[tokio::test]
async fn a_controller_can_join_another_mesh_and_keep_serving() {
    let dir = TempDir::new().expect("a temporary directory");
    let endpoint = endpoint().await;
    let fleet = Fleet::controller(endpoint.clone(), dir.path(), "build-box");
    fleet.run().await;

    let remote_dir = TempDir::new().expect("a temporary directory");
    let remote = self::endpoint().await;
    let desktop = Mesh::create(remote_dir.path(), remote.clone(), "desktop")
        .await
        .expect("the remote desktop makes its mesh");
    let (peer, join) = desktop.protocols();
    let router = beep_mesh::router(&remote)
        .accept(beep_mesh::PEER_ALPN, peer)
        .accept(beep_mesh::JOIN_ALPN, join)
        .spawn();
    let ticket = desktop
        .invite(DeviceKind::Desktop, DEFAULT_TICKET_TTL)
        .await
        .expect("a desktop ticket");

    fleet
        .join(&ticket, "build-box")
        .await
        .expect("the controller joins without closing its endpoint");
    assert!(!endpoint.is_closed());
    let mesh = fleet.ready().await.expect("the joined mesh stays open");
    assert_eq!(mesh.id(), desktop.id());
    assert!(mesh.is_desktop_observer(fleet.id()));
    assert!(
        mesh.code_hosts()
            .iter()
            .all(|host| host.device.id != fleet.id())
    );

    let node_dir = TempDir::new().expect("a temporary directory");
    let node_endpoint = self::endpoint().await;
    let ticket = fleet.invite(DeviceKind::Node).await.expect("a node ticket");
    let node = timeout(
        Duration::from_secs(20),
        Mesh::join(node_dir.path(), node_endpoint.clone(), &ticket, "gpu-box"),
    )
    .await
    .expect("the replacement router answers in time")
    .expect("the node joins through the controller");
    assert_eq!(node.id(), desktop.id());

    // A previous hosting build leaves no observer marker when removed.
    mesh.set(Key::Observer(fleet.id().to_string()), None)
        .await
        .expect("the previous build clears its observer marker");
    let mut updates = mesh.updates();
    desktop
        .remove(fleet.id())
        .await
        .expect("the remote desktop removes the controller");
    timeout(Duration::from_secs(20), async {
        while mesh.is_member(fleet.id()) {
            updates.changed().await.expect("the mesh stays open");
        }
    })
    .await
    .expect("the controller learns it was removed");
    assert!(matches!(fleet.ready().await, Err(DesktopError::NoMesh(_))));
    drop(mesh);
    drop(updates);
    fleet.stop().await;
    drop(fleet);

    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(endpoint.secret_key().clone())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the restarted controller binds");
    let fleet = Fleet::controller(endpoint.clone(), dir.path(), "build-box");
    fleet.run().await;
    assert!(
        fleet.mesh().is_some(),
        "a removed desktop can reopen across a hosting mode change"
    );
    assert!(matches!(fleet.ready().await, Err(DesktopError::NoMesh(_))));

    let ticket = desktop
        .invite(DeviceKind::Desktop, DEFAULT_TICKET_TTL)
        .await
        .expect("a replacement desktop ticket");
    fleet
        .join(&ticket, "build-box")
        .await
        .expect("a removed controller can join again");
    let mesh = fleet
        .ready()
        .await
        .expect("the controller is a member again");
    assert!(mesh.is_member(fleet.id()));
    assert!(mesh.is_desktop_observer(fleet.id()));

    fleet.stop().await;
    router.shutdown().await.expect("the desktop router stops");
    node_endpoint.close().await;
}
