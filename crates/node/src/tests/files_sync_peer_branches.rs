use super::*;
use crate::files::sync::{gitdir::Oid, peer::ALPN};
use crate::files::{
    Folders,
    sync::{map::Replica, project::Start},
    transfer::place::open_target,
};
use iroh::{
    Endpoint,
    endpoint::presets,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Exactly the old request variants and their field order.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum LegacyRequest {
    Handoff {
        project: String,
    },
    Pull {
        project: String,
        replica: Replica,
        since: u64,
        copy_git: bool,
    },
    GitObjects {
        project: String,
        commit: Oid,
        branch: String,
        have: Option<Oid>,
    },
}

#[derive(Debug, Clone, Default)]
struct LegacyPeer {
    rejected: Arc<AtomicUsize>,
    pulled: Arc<AtomicUsize>,
}

impl ProtocolHandler for LegacyPeer {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        while let Ok((send, recv)) = connection.accept_bi().await {
            let (mut tx, mut rx) = link(send, recv);
            match rx.recv::<LegacyRequest>().await {
                Ok(Some(LegacyRequest::Pull { .. })) => {
                    self.pulled.fetch_add(1, Ordering::SeqCst);
                    tx.send(&Reply::Head {
                        commit: None,
                        behind: None,
                        has_git: false,
                        branch: None,
                    })
                    .await
                    .expect("the legacy head is sent");
                    let _ = rx.recv::<LegacyRequest>().await;
                }
                Err(_) => {
                    self.rejected.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[test]
fn existing_pull_and_object_requests_keep_their_wire_bytes() {
    let old = LegacyRequest::Pull {
        project: "project".into(),
        replica: Replica::default(),
        since: 9,
        copy_git: false,
    };
    let new = Request::Pull {
        project: "project".into(),
        replica: Replica::default(),
        since: 9,
        copy_git: false,
    };
    assert_eq!(
        rmp_serde::to_vec(&old).unwrap(),
        rmp_serde::to_vec(&new).unwrap()
    );
    let old = LegacyRequest::GitObjects {
        project: "project".into(),
        commit: Oid::from([1; 32]),
        branch: "refs/heads/main".into(),
        have: None,
    };
    let new = Request::GitObjects {
        project: "project".into(),
        commit: Oid::from([1; 32]),
        branch: "refs/heads/main".into(),
        have: None,
    };
    assert_eq!(
        rmp_serde::to_vec(&old).unwrap(),
        rmp_serde::to_vec(&new).unwrap()
    );
}

#[tokio::test]
async fn new_pulls_fall_back_after_a_legacy_reader_rejects_the_new_variant() {
    let server = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let client = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let legacy = LegacyPeer::default();
    let router = Router::builder(server.clone())
        .accept(ALPN, legacy.clone())
        .spawn();
    let temp = tempfile::tempdir().unwrap();
    let base = crate::files::paths::canonicalize(temp.path()).unwrap();
    let folder = base.join("project");
    std::fs::create_dir(&folder).unwrap();
    let folders = Folders::new(std::slice::from_ref(&base));
    let root = open_target(&folders, &folder.to_string_lossy()).unwrap();
    let shared = Shared::load(Start {
        name: "project".into(),
        key: 1,
        root: Arc::new(root),
        peers: vec![server.addr()],
        file: base.join("state"),
        paused: false,
    })
    .unwrap();
    // A downgraded peer must not keep an earlier branch-aware advertisement.
    shared.observe_branches(&server.id(), Vec::new());
    let connection = client.connect(server.addr(), ALPN).await.unwrap();
    let (tx, mut rx, first) = timeout(
        std::time::Duration::from_secs(5),
        open_pull(&connection, &shared, &server.id()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(first.is_none());
    assert!(matches!(
        rx.recv::<Reply>().await.unwrap(),
        Some(Reply::Head { .. })
    ));
    assert_eq!(legacy.rejected.load(Ordering::SeqCst), 1);
    assert_eq!(legacy.pulled.load(Ordering::SeqCst), 1);
    assert!(!shared.branch_aware(&server.id()));
    drop(tx);
    drop(rx);
    client.close().await;
    router.shutdown().await.unwrap();
}
