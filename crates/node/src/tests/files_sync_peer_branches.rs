use super::*;
use crate::files::sync::{
    gitdir::{
        Oid,
        tests::{commit, init},
    },
    peer::ALPN,
};
use crate::files::{
    Folders,
    place::open_target,
    sync::{map::Replica, project::Start},
};
use iroh::{
    Endpoint,
    endpoint::presets,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
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
        haves: vec![Oid::from([2; 32])],
    };
    // Old peers read the request without the haves; new peers read old
    // requests with none.
    let read: LegacyRequest =
        beep_proto::lenient::from_slice(&rmp_serde::to_vec(&new).unwrap()).unwrap();
    assert_eq!(read, old);
    let read: Request = beep_proto::lenient::from_slice(&rmp_serde::to_vec(&old).unwrap()).unwrap();
    assert_eq!(
        read,
        Request::GitObjects {
            project: "project".into(),
            commit: Oid::from([1; 32]),
            branch: "refs/heads/main".into(),
            have: None,
            haves: Vec::new(),
        }
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
    let base = crate::fs::paths::canonicalize(temp.path()).unwrap();
    let folder = base.join("project");
    std::fs::create_dir(&folder).unwrap();
    let shared = test_project(&base, &folder, &server);
    // A downgraded peer must not keep an earlier branch-aware advertisement.
    shared.observe_branches(&server.id(), Vec::new());
    let connection = client.connect(server.addr(), ALPN).await.unwrap();
    let (tx, mut rx, first) = timeout(
        Duration::from_secs(5),
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

#[tokio::test]
async fn live_branch_pack_cut_short_leaves_the_checkout_unchanged_and_retries() {
    let temp = tempfile::tempdir().unwrap();
    let base = crate::fs::paths::canonicalize(temp.path()).unwrap();
    let target = base.join("project");
    let (source_dir, source) = init();
    let source_file = source_dir.path().join("tracked");
    fs::write(&source_file, "base").unwrap();
    source.run(&["add", "tracked"]).unwrap();
    commit(&source, "base");
    let cloned = Command::new("git")
        .args(["clone", "-q", "--no-local"])
        .arg(source_dir.path())
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );

    let repo = Repo::open(&target).unwrap();
    let tracked = target.join("tracked");
    fs::write(&tracked, "staged").unwrap();
    repo.run(&["add", "tracked"]).unwrap();
    fs::write(&tracked, "unstaged").unwrap();
    fs::write(target.join("untracked"), "untracked").unwrap();
    let checkout = || {
        (
            repo.head(),
            repo.head_ref(),
            fs::read(target.join(".git/index")).unwrap(),
            fs::read(&tracked).unwrap(),
            fs::read(target.join("untracked")).unwrap(),
        )
    };
    let before = checkout();
    let refs_before = repo.run(&["show-ref"]).unwrap().stdout;

    fs::write(&source_file, "peer edit").unwrap();
    source.run(&["add", "tracked"]).unwrap();
    commit(&source, "peer edit");
    let tips = source.branches().unwrap();
    let [tip] = tips.as_slice() else {
        panic!("the source has one branch");
    };
    let pack = source
        .run(&["pack-objects", "--stdout", "--all"])
        .unwrap()
        .stdout;
    let (first, rest) = pack.split_at(pack.len() / 2);

    let server = Endpoint::builder(presets::Minimal)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap();
    let client = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let shared = test_project(&base, &target, &server);
    let peer_ref = Repo::peer_ref(&server.id(), &tip.name).unwrap();

    let pack_dir = target.join(".git/objects/pack");
    let packs_before = fs::read_dir(&pack_dir).unwrap().count();
    let err = exchange_pack(&client, &server, &shared, tip, first, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("did not complete the Git pack"),
        "{err:#}"
    );
    assert_eq!(repo.run(&["show-ref"]).unwrap().stdout, refs_before);
    assert_eq!(checkout(), before);
    assert!(!repo.is_complete(tip.commit));
    assert_eq!(fs::read_dir(&pack_dir).unwrap().count(), packs_before);
    repo.run(&["fsck", "--connectivity-only"]).unwrap();

    exchange_pack(&client, &server, &shared, tip, first, Some(rest))
        .await
        .unwrap();
    assert_eq!(repo.ref_oid(&peer_ref), Some(tip.commit));
    assert!(repo.is_complete(tip.commit));
    assert_eq!(checkout(), before);
    repo.run(&["fsck", "--connectivity-only"]).unwrap();
    client.close().await;
    server.close().await;
}

fn test_project(base: &Path, folder: &Path, peer: &Endpoint) -> Arc<Shared> {
    let folders = Folders::new(&[base.to_owned()]);
    let root = open_target(&folders, &folder.to_string_lossy()).unwrap();
    Shared::load(Start {
        name: "project".into(),
        key: 1,
        root: Arc::new(root),
        peers: vec![peer.addr()],
        file: base.join("state"),
        paused: false,
        rules: String::new(),
        occupancy: crate::occupancy::Occupancy::default(),
    })
    .unwrap()
}

async fn exchange_pack(
    client: &Endpoint,
    server: &Endpoint,
    shared: &Shared,
    tip: &BranchTip,
    first: &[u8],
    rest: Option<&[u8]>,
) -> Result<()> {
    timeout(Duration::from_secs(15), async {
        let (connection, remote) = tokio::join!(client.connect(server.addr(), ALPN), async {
            server.accept().await.unwrap().await.unwrap()
        });
        let connection = connection.unwrap();
        let peer = server.id();
        let (result, ()) = tokio::join!(
            receive_catalog(&connection, shared, &peer, vec![tip.clone()]),
            async {
                let (send, recv) = remote.accept_bi().await.unwrap();
                let (mut tx, mut rx) = link(send, recv);
                assert!(matches!(
                    rx.recv::<Request>().await.unwrap(),
                    Some(Request::BranchObjects { commit, .. }) if commit == tip.commit
                ));

                tx.send(&Reply::GitChunk {
                    bytes: first.to_vec(),
                })
                .await
                .unwrap();
                if let Some(rest) = rest {
                    tx.send(&Reply::GitChunk {
                        bytes: rest.to_vec(),
                    })
                    .await
                    .unwrap();
                    tx.send(&Reply::GitDone).await.unwrap();
                }
                tx.finish().await.unwrap();
            }
        );
        remote.close(0u32.into(), b"test transfer ended");
        connection.closed().await;
        result
    })
    .await
    .expect("the Git pack exchange completes")
}
