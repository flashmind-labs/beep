use super::*;
use crate::files::paths;
use beep_mesh::{DeviceKind, JOIN_ALPN, PEER_ALPN, transport_config};
use iroh::{SecretKey, endpoint::presets, protocol::Router};
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
async fn a_removed_device_is_refused_on_a_list_the_server_set() {
    // A desktop's mesh with a node that joins it, and no `sync/` key:
    // the server sets the project list.
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
    let state = TempDir::new().expect("a temp folder");
    let here = bind().await;
    let syncthing = Syncthing::at(vec![state.path().join("syncthing")], None);
    let transfers = crate::files::transfer::Transfers::start_with(
        &here,
        state.path(),
        Folders::new(&[shared.path().to_owned()]),
        syncthing,
    )
    .await
    .expect("the transfers start");
    let syncs = transfers.syncs();
    syncs.follow(mesh.clone());
    let refused = syncs
        .configure(vec![Project {
            name: "proj".to_owned(),
            folder: paths::canonicalize(shared.path().join("proj"))
                .expect("the folder exists")
                .display()
                .to_string(),
            peers: vec![node.addr()],
            paused: false,
        }])
        .await;
    assert!(refused.is_empty(), "{refused:?}");
    let id = node.id();
    assert!(syncs.inner.may_connect(&id), "a peer of the list connects");

    mesh.remove(id).await.expect("the removal is signed");
    assert!(!syncs.inner.may_connect(&id), "a removed device does not");
    joined.close().await.ok();
    transfers.shutdown().await.expect("the node stops");
}

#[test]
fn a_project_in_another_folder_keeps_other_state() {
    let a = key_of("game", Path::new("/srv/project"));
    assert_eq!(a, key_of("game", Path::new("/srv/project")));
    assert_ne!(a, key_of("game", Path::new("/srv/other")));
    assert_ne!(a, key_of("gam", Path::new("e/srv/project")));
}

#[test]
fn a_missing_folder_is_made_only_where_the_folder_above_it_is() {
    let shared = TempDir::new().expect("a temp folder");
    let base = paths::canonicalize(shared.path()).expect("it is there");
    let folders = Folders::new(std::slice::from_ref(&base));
    let here = base.join("game");
    let made = make_missing(&folders, &here.to_string_lossy()).expect("it is made");
    assert!(made && here.is_dir());
    let again = make_missing(&folders, &here.to_string_lossy()).expect("it is there");
    assert!(!again);

    // A disk that is not mounted leaves no folder above the project's.
    let unmounted = base.join("disk").join("game");
    let said = make_missing(&folders, &unmounted.to_string_lossy()).expect_err("nothing is made");
    assert!(format!("{said:#}").starts_with(PARENT_MISSING), "{said:#}");
    assert!(!base.join("disk").exists());
}
