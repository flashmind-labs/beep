//! What a sync peer says to another: the requests and replies of a pull, and
//! the pull of a Git pack.

use super::super::gitdir::BranchTip;
use super::super::{
    gitdir::{Checkout, Oid, Repo},
    map::{Entry, Replica},
    parts::STALL,
    project::Shared,
};
use anyhow::{Context, Result, bail};
use beep_mesh::OPEN_TIMEOUT;
use beep_proto::node::link;
use iroh::endpoint::Connection;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::{io::AsyncWriteExt, task::spawn_blocking, time::timeout};

/// How long a Git object transfer waits for its first chunk, then for each
/// next one.
const FIRST_CHUNK: Duration = Duration::from_secs(300);
/// The largest Git pack one transfer takes.
const MAX_PACK: u64 = 2 << 30;

/// Sent as compact msgpack, like the node link, so a field is added only at
/// the end with `#[serde(default)]`. The same goes for `Reply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub(super) enum Request {
    Handoff {
        project: String,
    },
    /// Sends `project`'s changes after `since`, when `replica` is the
    /// peer's replica, or all of them, then each change as it happens.
    Pull {
        project: String,
        replica: Replica,
        since: u64,
        #[serde(default)]
        copy_git: bool,
    },
    GitObjects {
        project: String,
        commit: Oid,
        branch: String,
        have: Option<Oid>,
        /// More commits the puller has, beyond `have`, for the pack to leave out.
        #[serde(default)]
        haves: Vec<Oid>,
    },
    /// The branch catalogue reply is sent only in response to this request.
    PullBranches {
        project: String,
        replica: Replica,
        since: u64,
        copy_git: bool,
    },
    BranchObjects {
        project: String,
        commit: Oid,
        branch: String,
        have: Option<Oid>,
        #[serde(default)]
        haves: Vec<Oid>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub(super) enum Reply {
    Handoff {
        snapshot: beep_proto::node::sync::HandoffSnapshot,
    },
    /// Sent first: the commit the peer's checkout is at, if it has one.
    Head {
        commit: Option<Oid>,
        /// The peer's last change the puller lacks, if any: the puller has
        /// all the peer had once its changes reach it. Peers before this
        /// field leave it out.
        #[serde(default)]
        behind: Option<u64>,
        #[serde(default)]
        has_git: bool,
        #[serde(default)]
        branch: Option<String>,
    },
    /// Changes of `replica` up to its change `upto`.
    Changes {
        replica: Replica,
        entries: Vec<Entry>,
        upto: u64,
        checkout: Checkout,
    },
    Refused {
        message: String,
    },
    GitChunk {
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    GitDone,
    Branches {
        tips: Vec<BranchTip>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum ObjectScope {
    Checkout,
    Branch,
}

pub(super) struct ObjectWant {
    pub(super) commit: Oid,
    pub(super) branch: String,
    /// Commits the puller has, the closest first.
    pub(super) haves: Vec<Oid>,
    pub(super) scope: ObjectScope,
}

pub(super) async fn receive_objects(
    connection: &Connection,
    shared: &Shared,
    want: ObjectWant,
) -> Result<()> {
    let (send, recv) = timeout(OPEN_TIMEOUT, connection.open_bi())
        .await
        .context("The Git object transfer did not start")??;
    let (mut tx, mut rx) = link(send, recv);
    let ObjectWant {
        commit,
        branch,
        haves,
        scope,
    } = want;
    // Peers before `haves` read only `have`: the closest commit.
    let have = haves.first().copied();
    let request = match scope {
        ObjectScope::Checkout => Request::GitObjects {
            project: shared.name().to_owned(),
            commit,
            branch,
            have,
            haves,
        },
        ObjectScope::Branch => Request::BranchObjects {
            project: shared.name().to_owned(),
            commit,
            branch,
            have,
            haves,
        },
    };
    tx.send(&request).await?;

    let root = shared.root().to_owned();
    let (repo, pack) = spawn_blocking(move || -> Result<_> {
        let repo = Repo::open(&root).context("Git checkout is unavailable")?;
        let pack = repo.incoming_pack()?;
        Ok((repo, pack))
    })
    .await??;
    let mut file = tokio::fs::File::create(pack.received()).await?;
    let mut total = 0u64;

    loop {
        // Git writes nothing until it has counted and compressed the
        // objects, which takes long for a big pack.
        let wait = if total == 0 { FIRST_CHUNK } else { STALL };
        let reply = timeout(wait, rx.recv::<Reply>())
            .await
            .context("The Git object transfer stalled")??;

        match reply {
            Some(Reply::GitChunk { bytes }) => {
                total = total
                    .checked_add(bytes.len() as u64)
                    .context("Git pack too large")?;
                if total > MAX_PACK {
                    bail!("Git pack exceeds the transfer limit");
                }

                file.write_all(&bytes).await?;
            }
            Some(Reply::GitDone) => break,
            Some(Reply::Refused { message }) => bail!("{message}"),
            _ => bail!("The peer did not complete the Git pack"),
        }
    }

    file.flush().await?;
    drop(file);
    // Indexing runs to its end even when the pull is dropped meanwhile, so
    // Git never leaves a half-written pack of its own.
    spawn_blocking(move || repo.index_pack(&pack)).await?
}

#[cfg(test)]
#[path = "../../../tests/files_sync_peer.rs"]
mod tests;
