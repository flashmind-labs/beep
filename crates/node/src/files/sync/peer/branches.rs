//! Negotiating branch-aware pulls without changing the existing pull messages.

use super::wire::{ObjectScope, ObjectWant, Reply, Request, receive_objects};
use crate::files::sync::{
    gitdir::{BranchTip, Repo},
    project::Shared,
};
use anyhow::{Context, Result, ensure};
use beep_proto::node::{LinkRx, LinkTx, link};
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

        match timeout(beep_mesh::OPEN_TIMEOUT, rx.recv::<Reply>()).await {
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

    // Only the branches this checkout takes, and of them only the tips that
    // moved since the last catalog, cost Git work.
    let (root, peer_id) = (shared.root().to_owned(), *peer);
    let (tips, changed) = spawn_blocking(move || -> Result<_> {
        let repo = Repo::open(&root).context("Git checkout is unavailable")?;
        let tips = tips
            .into_iter()
            .filter(|tip| repo.takes_branch(&tip.name))
            .collect::<Vec<_>>();
        let changed = repo.prepare_peer_refs(&peer_id, &tips)?;
        Ok((tips, changed))
    })
    .await??;

    for tip in &changed {
        ensure!(
            shared.has_peer(peer) && !shared.is_stopped(),
            "Project sharing stopped"
        );
        let (root, checked_tip, peer_id) = (shared.root().to_owned(), tip.clone(), *peer);
        let (need, haves) = spawn_blocking(move || -> Result<_> {
            let repo = Repo::open(&root).context("Git checkout is unavailable")?;
            Ok((
                !repo.is_complete(checked_tip.commit),
                repo.haves(&peer_id, &checked_tip.name),
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
                    haves,
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

    if !changed.is_empty() {
        let root = shared.root().to_owned();
        spawn_blocking(move || Repo::open(&root).map(|repo| repo.collect())).await?;
    }

    shared.observe_branches(peer, tips);
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/files_sync_peer_branches.rs"]
mod tests;
