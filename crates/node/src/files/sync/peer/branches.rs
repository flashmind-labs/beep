//! Negotiating branch-aware pulls without changing the existing pull messages.

use super::{
    BranchTip, ObjectScope, ObjectWant, Reply, Repo, Request, Shared, link, receive_objects,
};
use anyhow::{Context, Result, ensure};
use beep_proto::node::{LinkRx, LinkTx};
use iroh::{EndpointId, endpoint::Connection};
use tokio::{task::spawn_blocking, time::timeout};

pub(super) async fn catalog(shared: &Shared) -> Result<Vec<BranchTip>> {
    let root = shared.root().to_owned();
    spawn_blocking(move || match Repo::open(&root) {
        Some(repo) => repo.branches(),
        None => Ok(Vec::new()),
    })
    .await?
}

pub(super) async fn open_pull(
    connection: &Connection,
    shared: &Shared,
    peer: &EndpointId,
) -> Result<(LinkTx, LinkRx, Option<Reply>)> {
    let cursor = shared.cursor(peer);
    for with_branches in [true, false] {
        let (send, recv) = connection.open_bi().await?;
        let (mut tx, mut rx) = link(send, recv);
        let request = if with_branches {
            Request::PullBranches {
                project: shared.name().to_owned(),
                replica: cursor.replica,
                since: cursor.seq,
                copy_git: shared.copies_git(),
            }
        } else {
            Request::Pull {
                project: shared.name().to_owned(),
                replica: cursor.replica,
                since: cursor.seq,
                copy_git: shared.copies_git(),
            }
        };
        tx.send(&request).await?;
        if !with_branches {
            return Ok((tx, rx, None));
        }
        match timeout(super::CONNECT_TIMEOUT, rx.recv::<Reply>()).await {
            Ok(Ok(Some(reply))) => {
                if !matches!(reply, Reply::Branches { .. }) {
                    shared.forget_branches(peer);
                }
                return Ok((tx, rx, Some(reply)));
            }
            // Old nodes reject the new request on this stream. Open a legacy pull.
            _ => shared.forget_branches(peer),
        }
    }
    unreachable!("the legacy pull always returns")
}

pub(super) async fn receive_catalog(
    connection: &Connection,
    shared: &Shared,
    peer: &EndpointId,
    tips: Vec<BranchTip>,
) -> Result<()> {
    BranchTip::validate(&tips)?;
    if shared.copies_git() || !shared.head().await?.present {
        // Bootstrap owns the initial copy; never index a pack into a partial repository.
        shared.observe_branches(peer, Vec::new());
        return Ok(());
    }
    let (root, peer_id, advertised) = (shared.root().to_owned(), *peer, tips.clone());
    spawn_blocking(move || {
        Repo::open(&root)
            .context("Git checkout is unavailable")?
            .prepare_peer_refs(&peer_id, &advertised)
    })
    .await??;
    for tip in &tips {
        ensure!(
            shared.has_peer(peer) && !shared.is_stopped(),
            "Project sharing stopped"
        );
        let (root, checked_tip) = (shared.root().to_owned(), tip.clone());
        let (need, have) = spawn_blocking(move || -> Result<_> {
            let repo = Repo::open(&root).context("Git checkout is unavailable")?;
            repo.validate_branch(&checked_tip.name)?;
            Ok((
                !repo.is_complete(checked_tip.commit),
                repo.ref_oid(&checked_tip.name),
            ))
        })
        .await??;
        if need {
            receive_objects(
                connection,
                shared,
                ObjectWant {
                    commit: tip.commit,
                    branch: tip.name.clone(),
                    have,
                    scope: ObjectScope::Branch,
                },
            )
            .await?;
        }
        let (root, tip, peer) = (shared.root().to_owned(), tip.clone(), *peer);
        spawn_blocking(move || {
            Repo::open(&root)
                .context("Git checkout is unavailable")?
                .keep_peer_branch(&peer, &tip)
        })
        .await??;
    }
    shared.observe_branches(peer, tips);
    Ok(())
}

#[cfg(test)]
mod tests {
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
            commit: Oid::from_bytes([1; 32]),
            branch: "refs/heads/main".into(),
            have: None,
        };
        let new = Request::GitObjects {
            project: "project".into(),
            commit: Oid::from_bytes([1; 32]),
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
}
