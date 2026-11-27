use super::*;
use crate::files::transfer::Transfers;
use crate::{
    files::{Folders, syncthing::Syncthing},
    fs::paths,
    membership::Membership,
};
use beep_mesh::{DeviceKind, JOIN_ALPN, Mesh, PEER_ALPN, transport_config};
use beep_proto::node::sync::Project;
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use iroh_blobs::{Hash, HashAndFormat};
use std::time::Duration;
use tempfile::TempDir;

async fn bind() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("an endpoint binds")
}

#[tokio::test]
async fn the_gate_asks_sync_before_the_grants() {
    let desktop_dir = TempDir::new().expect("a temp folder");
    let desktop = bind().await;
    let mesh = Mesh::create(desktop_dir.path(), desktop.clone(), "desktop")
        .await
        .expect("the mesh is made");
    let (peer, join) = mesh.protocols();
    let _router = Router::builder(desktop)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn();
    let ticket = mesh
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .expect("a ticket");
    let (node_dir, node) = (TempDir::new().expect("a temp folder"), bind().await);
    let joined = Mesh::join(node_dir.path(), node.clone(), &ticket, "node")
        .await
        .expect("the node joins");
    let shared = TempDir::new().expect("a temp folder");
    std::fs::create_dir(shared.path().join("proj")).expect("a project folder");
    std::fs::write(shared.path().join("proj").join("a.txt"), "synced bytes").expect("a file");
    let state = TempDir::new().expect("a temp folder");
    let membership = Membership::default();
    membership.set(mesh.clone());
    let (transfers, _served) = Transfers::build(
        &bind().await,
        state.path(),
        Folders::new(&[shared.path().to_owned()]),
        Syncthing::at(vec![state.path().join("syncthing")], None),
        membership,
        false,
    )
    .await
    .expect("the transfers start");
    let syncs = transfers.syncs();
    let refused = syncs
        .configure_from_mesh(vec![Project {
            name: "proj".to_owned(),
            folder: paths::canonicalize(shared.path().join("proj"))
                .expect("the folder exists")
                .display()
                .to_string(),
            peers: vec![node.addr()],
            paused: false,
            rules: String::new(),
        }])
        .await;
    assert!(refused.is_empty(), "{refused:?}");

    syncs.rescan("proj").await.expect("the project scans");

    let grants = Grants::new();
    let member = node.id();
    let stranger = SecretKey::generate().public();
    // A peer of a synced project connects with no grant, a stranger does not.
    assert!(connect(&grants, &syncs, 1, Some(member)).is_ok());
    assert!(connect(&grants, &syncs, 2, Some(stranger)).is_err());
    assert!(connect(&grants, &syncs, 3, None).is_err());

    // With no grant, it gets the project's blobs and only raw.
    let held = Hash::new(b"synced bytes");
    assert!(get(&grants, &syncs, 1, &GetRequest::blob(held)).is_ok());
    assert!(get(&grants, &syncs, 1, &GetRequest::all(held)).is_err());
    assert!(get(&grants, &syncs, 1, &GetRequest::blob(Hash::new(b"other"))).is_err());

    // A removed device is refused even with an open offer for it, and one
    // already connected loses its requests.
    let file = Hash::new(b"offered");
    let arc = Arc::new(grants);
    let _guard = arc.grant(member, HashAndFormat::raw(file));
    assert!(get(&arc, &syncs, 1, &GetRequest::blob(file)).is_ok());
    mesh.remove(member).await.expect("the removal is signed");
    assert!(get(&arc, &syncs, 1, &GetRequest::blob(file)).is_err());
    assert!(connect(&arc, &syncs, 4, Some(member)).is_err());

    joined.close().await.ok();
    transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_fetch_counts_the_bytes_sent_of_every_blob() {
    use beep_proto::watched;
    use iroh_blobs::provider::events::{TransferProgress, TransferStarted};

    let (tx, rx) = irpc::channel::mpsc::channel(8);
    let (sent, watched) = watch::channel(0);
    let counting = tokio::spawn(count(rx, Some(sent)));
    let started = |index, size| TransferStarted {
        index,
        hash: Hash::new(b"blob"),
        size,
    };

    tx.send(started(0, 64).into()).await.unwrap();
    tx.send(TransferProgress { end_offset: 64 }.into())
        .await
        .unwrap();
    tx.send(started(1, 1000).into()).await.unwrap();
    tx.send(TransferProgress { end_offset: 400 }.into())
        .await
        .unwrap();
    drop(tx);
    counting.await.unwrap();

    assert_eq!(watched::read(&watched, |bytes| *bytes), 464);
}
