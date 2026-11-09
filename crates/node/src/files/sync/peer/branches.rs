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
#[path = "../../../tests/files_sync_peer_branches.rs"]
mod tests;
