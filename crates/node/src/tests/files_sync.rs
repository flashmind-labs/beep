use super::*;
use crate::fs::paths;
use beep_mesh::Mesh;
use beep_mesh::{DeviceKind, JOIN_ALPN, PEER_ALPN, transport_config};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
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
async fn a_removed_device_is_refused_and_mesh_owns_configure() {
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
    syncs.inner.mesh.set(mesh.clone());
    let project = Project {
        name: "proj".to_owned(),
        folder: paths::canonicalize(shared.path().join("proj"))
            .expect("the folder exists")
            .display()
            .to_string(),
        peers: vec![node.addr()],
        paused: false,
        rules: String::new(),
    };
    let reply = answer(
        &syncs,
        Request::Configure {
            projects: vec![project.clone()],
        },
    )
    .await
    .expect("answered");
    assert_eq!(
        reply,
        Reply::Configured {
            addr: syncs.addr(),
            refused: vec![Refused {
                project: "proj".to_owned(),
                message: "this machine takes its projects from its mesh".to_owned(),
            }],
        }
    );

    let refused = syncs.configure_from_mesh(vec![project]).await;
    assert!(refused.is_empty(), "{refused:?}");
    let id = node.id();
    assert!(syncs.inner.may_connect(&id), "a peer of the list connects");

    mesh.remove(id).await.expect("the removal is signed");
    assert!(!syncs.inner.may_connect(&id), "a removed device does not");

    // Leaving the mesh clears the one copy: no mesh, nobody is removed.
    syncs.inner.mesh.clear();
    assert!(syncs.inner.allows(&id), "no mesh removes nobody");
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

#[tokio::test]
async fn a_project_handed_to_syncthing_forgets_its_state() {
    let shared = TempDir::new().expect("a temp folder");
    let base = paths::canonicalize(shared.path()).expect("it is there");
    let folder = base.join("proj");
    std::fs::create_dir(&folder).expect("a project folder");
    std::fs::write(folder.join("a.txt"), "one").expect("a file");
    let state = TempDir::new().expect("a temp folder");
    let transfers = crate::files::transfer::Transfers::start_with(
        &bind().await,
        state.path(),
        Folders::new(std::slice::from_ref(&base)),
        Syncthing::at(Vec::new(), Some(base.clone())),
    )
    .await
    .expect("the transfers start");
    let syncs = transfers.syncs();
    let project = Project {
        name: "proj".to_owned(),
        folder: folder.display().to_string(),
        peers: Vec::new(),
        paused: false,
        rules: String::new(),
    };
    let refused = syncs.configure(vec![project]).await;
    assert!(refused.is_empty(), "{refused:?}");

    // Syncthing starts keeping the folder in step.
    std::fs::write(base.join(".stfolder"), "").expect("a marker");
    syncs
        .syncthing_files(&folder.display().to_string(), Some(true))
        .await
        .ok();

    assert!(syncs.held().contains("proj"));
    let states = std::fs::read_dir(&syncs.inner.dir)
        .expect("the state folder")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "state"))
        .count();
    assert_eq!(states, 0, "a held project keeps no state");
    transfers.shutdown().await.expect("the node stops");
}
