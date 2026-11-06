use super::*;
use crate::files::syncthing::Syncthing;
use beep_mesh::transport_config;
use beep_proto::node::sync::Project;
use iroh::{SecretKey, endpoint::presets};
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
        false,
    )
    .await
    .expect("the transfers start");
    let router = served
        .add_to(Router::builder(here.clone()).accept(b"hung", Hung))
        .spawn();
    let folder = shared.path().join("proj").canonicalize().expect("exists");
    let refused = transfers
        .syncs()
        .configure(vec![Project {
            name: "proj".to_owned(),
            folder: folder.display().to_string(),
            peers: vec![peer.addr()],
            paused: false,
        }])
        .await;
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(transfers.syncs().status().len(), 1);

    let stopped = timeout(QUIT_LIMIT, stop_for_exit(&router, Some(&transfers))).await;
    assert!(stopped.is_err(), "the goodbye still hangs at the limit");
    // The project stopped and saved in spite of it.
    assert!(transfers.syncs().status().is_empty());
}
