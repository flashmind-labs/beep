use super::*;
use crate::fs::paths;
use crate::files::syncthing::Syncthing;
use beep_mesh::transport_config;
use beep_proto::node::sync::Project;
use iroh::{SecretKey, endpoint::presets};
use iroh_blobs::HashAndFormat;
use std::future::pending;
use tempfile::TempDir;

/// What quitting gives the whole stop in the Mac app.
const QUIT_LIMIT: Duration = Duration::from_millis(300);

async fn bind() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("an endpoint binds")
}

/// A protocol whose goodbye never ends, as with a peer that stopped
/// answering.
#[derive(Debug)]
struct Hung;

impl ProtocolHandler for Hung {
    async fn accept(&self, _connection: Connection) -> Result<(), AcceptError> {
        Ok(())
    }

    async fn shutdown(&self) {
        pending::<()>().await;
    }
}

#[tokio::test]
async fn quitting_saves_the_projects_while_a_peer_hangs() {
    let shared = TempDir::new().expect("a temp folder");
    fs::create_dir(shared.path().join("proj")).expect("a project folder");
    let state = TempDir::new().expect("a temp folder");
    let here = bind().await;
    let peer = bind().await;
    let syncthing = Syncthing::at(vec![state.path().join("syncthing")], None);
    let (transfers, served) = Transfers::build(
        &here,
        state.path(),
        Folders::new(&[shared.path().to_owned()]),
        syncthing,
        Membership::default(),
        false,
    )
    .await
    .expect("the transfers start");
    let router = served
        .add_to(Router::builder(here.clone()).accept(b"hung", Hung))
        .spawn();
    let folder = paths::canonicalize(shared.path().join("proj")).expect("exists");
    let refused = transfers
        .syncs()
        .configure(vec![Project {
            name: "proj".to_owned(),
            folder: folder.display().to_string(),
            peers: vec![peer.addr()],
            paused: false,
            rules: String::new(),
        }])
        .await;
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(transfers.syncs().status().len(), 1);

    let stopped = timeout(QUIT_LIMIT, stop_for_exit(&router, Some(&transfers))).await;
    assert!(stopped.is_err(), "the goodbye still hangs at the limit");
    // The project stopped and saved in spite of it.
    assert!(transfers.syncs().status().is_empty());
}

#[tokio::test]
async fn a_shared_file_is_fetched_by_the_device_that_asked_and_no_other() {
    let files = TempDir::new().expect("a temp folder");
    let state = TempDir::new().expect("a temp folder");
    let path = files.path().join("clip.mp4");
    let data = b"frames".repeat(50_000);
    fs::write(&path, &data).expect("the file is written");
    let here = bind().await;
    let asking = bind().await;
    let other = bind().await;
    let syncthing = Syncthing::at(vec![state.path().join("syncthing")], None);
    let (transfers, _served) = Transfers::build(
        &here,
        state.path(),
        Folders::new(&[]),
        syncthing,
        Membership::default(),
        true,
    )
    .await
        .expect("the transfers start");

    let shared = transfers
        .share(&path, asking.id())
        .await
        .expect("the file is shared");
    assert_eq!(shared.size, data.len() as u64);
    let content = HashAndFormat::raw(Hash::from_bytes(shared.hash));

    let store = iroh_blobs::store::mem::MemStore::new();
    let connection = asking
        .connect(here.addr(), iroh_blobs::ALPN)
        .await
        .expect("the asking device connects");
    store
        .remote()
        .fetch(connection, content)
        .await
        .expect("the asking device fetches it");
    let fetched = store.blobs().get_bytes(content.hash).await.expect("stored");
    assert_eq!(fetched.as_ref(), data.as_slice());

    // Sharing again renews the lease and gives the same blob.
    let again = transfers.share(&path, asking.id()).await.expect("shared again");
    assert_eq!(again, shared);

    let refused = iroh_blobs::store::mem::MemStore::new();
    let fetched = match other.connect(here.addr(), iroh_blobs::ALPN).await {
        Ok(connection) => refused.remote().fetch(connection, content).await.is_ok(),
        Err(_) => false,
    };
    assert!(!fetched, "a device that did not ask fetches nothing");
    transfers.shutdown().await.expect("the transfers stop");
}
