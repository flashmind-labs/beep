use super::*;

/// `Reply::Head` as peers from before `behind` have it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum OldReply {
    Head { commit: Option<Oid> },
}

#[test]
fn head_stays_readable_both_ways_within_the_alpn() {
    let new = rmp_serde::to_vec(&Reply::Head {
        commit: None,
        behind: Some(7),
        has_git: true,
        branch: None,
    })
    .unwrap();
    assert_eq!(
        beep_proto::lenient::from_slice::<OldReply>(&new).unwrap(),
        OldReply::Head { commit: None }
    );
    let old = rmp_serde::to_vec(&OldReply::Head { commit: None }).unwrap();
    assert_eq!(
        beep_proto::lenient::from_slice::<Reply>(&old).unwrap(),
        Reply::Head {
            commit: None,
            behind: None,
            has_git: false,
            branch: None,
        }
    );
}

/// A peer that answers every pull with its head, then refuses, as a pull
/// that fails on a Git wait does after `Head`.
#[derive(Debug, Clone)]
struct RefusesAfterHead {
    behind: Option<u64>,
}

impl ProtocolHandler for RefusesAfterHead {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        while let Ok((send, recv)) = connection.accept_bi().await {
            let (mut tx, mut rx) = link(send, recv);

            if rx.recv::<Request>().await.is_err() {
                continue;
            }

            let head = Reply::Head {
                commit: None,
                behind: self.behind,
                has_git: false,
                branch: None,
            };
            let refused = Reply::Refused {
                message: "waiting for Git".into(),
            };
            if tx.send(&head).await.is_ok() {
                let _ = tx.send(&refused).await;
            }
            let _ = rx.recv::<Request>().await;
        }

        Ok(())
    }
}

#[tokio::test]
async fn a_pull_that_fails_after_head_keeps_backing_off() {
    use crate::files::{
        Folders,
        sync::{Parts, project::Start, watch::Watcher},
        transfer::place::{Journal, open_target},
    };
    use iroh::endpoint::presets;
    use iroh::protocol::Router;
    use iroh_blobs::store::fs::FsStore;

    let temp = tempfile::tempdir().unwrap();
    let base = crate::files::paths::canonicalize(temp.path()).unwrap();
    let folder = base.join("project");
    std::fs::create_dir(&folder).unwrap();
    let folders = Folders::new(std::slice::from_ref(&base));
    let root = open_target(&folders, &folder.to_string_lossy()).unwrap();
    let client = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let parts = Parts {
        endpoint: client.clone(),
        store: FsStore::load(base.join("store")).await.unwrap(),
        store_dir: base.join("store"),
        journal: Journal::open(base.join("partial")).unwrap(),
        room: Arc::default(),
    };
    let shared = Shared::load(Start {
        name: "project".into(),
        key: 1,
        root: Arc::new(root),
        peers: Vec::new(),
        file: base.join("state"),
        paused: false,
        rules: String::new(),
    })
    .unwrap();
    // The task answers the scan a pull asks for before matching the peer.
    let task = tokio::spawn(super::super::project::run(
        shared.clone(),
        parts.clone(),
        Watcher::with_budget(None, None),
    ));

    for (behind, after) in [(Some(3), LAST_RETRY), (None, FIRST_RETRY)] {
        let server = Endpoint::builder(presets::Minimal).bind().await.unwrap();
        let router = Router::builder(server.clone())
            .accept(ALPN, RefusesAfterHead { behind })
            .spawn();
        let mut wait = LAST_RETRY;

        let pulled = timeout(
            Duration::from_secs(30),
            pull_once(&shared, &client, &server.addr(), &mut wait),
        )
        .await
        .unwrap();

        assert!(pulled.is_err());
        assert_eq!(wait, after, "behind {behind:?}");
        router.shutdown().await.unwrap();
    }

    shared.stop();
    task.await.unwrap();
    parts.store.shutdown().await.ok();
    client.close().await;
}
