//! Node-to-node sync on `ALPN`: `pull` reads a peer's project lists directly
//! over iroh, so sync goes on while the server is down, and `PeerHandler`
//! serves them to each project's peers only.

use super::{
    Syncs,
    gitdir::Oid,
    map::{Entry, Replica},
    project::{Cursor, Shared},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{LinkRx, LinkTx, link};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc, time::Duration};
use tokio::{
    task::JoinSet,
    time::{Instant, sleep, timeout},
};

/// `beep-sync/1` nodes leave `.git` and tracked files out and would write
/// tracked files into a checkout git owns there, so the two do not sync with
/// each other: a node syncs again once both run this version.
pub(crate) const ALPN: &[u8] = b"beep-sync/2";

/// The most entries one batch carries.
const BATCH: usize = 1000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);
/// The most projects one peer pulls at once over a connection.
const MAX_STREAMS: usize = 64;

/// Sent as msgpack maps with named fields, like the node link, so nodes of
/// different versions can add fields. The same goes for `Reply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
enum Request {
    /// Sends `project`'s changes after `since`, when `replica` is the
    /// peer's replica, or all of them, then each change as it happens.
    Pull {
        project: String,
        replica: Replica,
        since: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
enum Reply {
    /// Sent first: the commit the peer's checkout is at, if it has one.
    Head {
        commit: Option<Oid>,
        /// The peer's last change the puller lacks, if any: the puller has
        /// all the peer had once its changes reach it. Peers before this
        /// field leave it out.
        #[serde(default)]
        behind: Option<u64>,
    },
    /// Changes of `replica` up to its change `upto`.
    Changes {
        replica: Replica,
        entries: Vec<Entry>,
        upto: u64,
    },
    Refused {
        message: String,
    },
}

/// Answers other nodes' pulls.
#[derive(Clone)]
pub(crate) struct PeerHandler {
    pub(crate) syncs: Syncs,
}

impl fmt::Debug for PeerHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerHandler").finish()
    }
}

impl ProtocolHandler for PeerHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let peer = connection.remote_id();
        if !self.syncs.is_peer(&peer) {
            log::info!("refused a sync connection from {peer}: not a peer");
            connection.close(1u32.into(), b"not a peer");
            return Ok(());
        }
        // The streams end with the connection: dropping the set stops them.
        let mut streams = JoinSet::new();
        loop {
            tokio::select! {
                stream = connection.accept_bi() => {
                    let Ok((send, recv)) = stream else {
                        return Ok(());
                    };
                    if streams.len() >= MAX_STREAMS {
                        log::warn!("refused a pull from {peer}: {MAX_STREAMS} open");
                        continue;
                    }
                    let (tx, rx) = link(send, recv);
                    streams.spawn(serve(self.syncs.clone(), peer, tx, rx));
                }
                Some(ended) = streams.join_next() => {
                    match ended {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => log::debug!("a pull from {peer} ended: {e:#}"),
                        Err(e) => log::warn!("a pull task failed: {e}"),
                    }
                    if !self.syncs.is_peer(&peer) {
                        connection.close(1u32.into(), b"not a peer");
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn serve(syncs: Syncs, peer: iroh::EndpointId, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(Request::Pull {
        project,
        replica,
        since,
    }) = rx.recv::<Request>().await?
    else {
        return Ok(());
    };
    let Some(shared) = syncs
        .project(&project)
        .filter(|shared| shared.has_peer(&peer) && syncs.allows(&peer))
    else {
        let message = format!("{project} is not synced with this node");
        return tx.send(&Reply::Refused { message }).await;
    };
    let _serving = syncs.serving();
    let mut since = if replica == shared.replica() {
        since
    } else {
        0
    };
    tx.send(&Reply::Head {
        commit: shared.head().await?,
        behind: shared.last_change_after(since),
    })
    .await?;
    let mut seq = shared.subscribe();
    let mut peers = shared.watch_peers();
    loop {
        // A device removed meanwhile gets nothing more, not even what is
        // already queued.
        if !syncs.allows(&peer) {
            return Ok(());
        }
        let changes = shared.changes(since, BATCH);
        if !changes.entries.is_empty() {
            since = changes.upto;
            tx.send(&Reply::Changes {
                replica: shared.replica(),
                entries: changes.entries,
                upto: changes.upto,
            })
            .await?;
            continue;
        }
        tokio::select! {
            changed = seq.changed() => if changed.is_err() {
                return Ok(());
            },
            () = shared.stopped() => return Ok(()),
            // A node taken out of the project gets nothing more.
            changed = peers.changed() => if changed.is_err() || !shared.has_peer(&peer) || !syncs.allows(&peer) {
                return Ok(());
            },
            // The peer sends nothing more; it closing ends the pull.
            _ = rx.recv::<Request>() => return Ok(()),
        }
    }
}

/// Pulls `shared`'s list from `peer` until the project stops, reconnecting
/// with growing waits.
pub(super) async fn pull(shared: Arc<Shared>, endpoint: Endpoint, peer: EndpointAddr) {
    let mut wait = FIRST_RETRY;
    loop {
        let pulled = tokio::select! {
            pulled = pull_once(&shared, &endpoint, &peer, &mut wait) => pulled,
            () = shared.stopped() => return,
        };
        match pulled {
            Ok(()) => wait = FIRST_RETRY,
            Err(e) => log::debug!("pulling {} from {}: {e:#}", shared.name(), peer.id),
        }
        shared.heard(&peer.id);
        tokio::select! {
            () = sleep(wait) => {}
            () = shared.stopped() => return,
        }
        wait = (wait * 2).min(LAST_RETRY);
    }
}

async fn pull_once(
    shared: &Shared,
    endpoint: &Endpoint,
    peer: &EndpointAddr,
    wait: &mut Duration,
) -> Result<()> {
    let started = Instant::now();
    let connection = timeout(CONNECT_TIMEOUT, endpoint.connect(peer.clone(), ALPN))
        .await
        .context("could not reach the peer in time")??;
    let (send, recv) = connection.open_bi().await?;
    let (mut tx, mut rx) = link(send, recv);
    let cursor = shared.cursor(&peer.id);
    tx.send(&Request::Pull {
        project: shared.name().to_owned(),
        replica: cursor.replica,
        since: cursor.seq,
    })
    .await?;
    let mut last = None;
    while let Some(reply) = rx.recv::<Reply>().await? {
        match reply {
            Reply::Head { commit, behind } => {
                // A connection that worked: losing it is no reason to wait long.
                *wait = FIRST_RETRY;
                log::debug!(
                    "{}: {} answered {:?} after dialling",
                    shared.name(),
                    peer.id,
                    started.elapsed()
                );
                shared.first_contact(&peer.id, commit).await?;
                last = behind;
                if last.is_none() {
                    shared.heard(&peer.id);
                }
            }
            Reply::Changes {
                replica,
                entries,
                upto,
            } => {
                shared.merge_from(&peer.id, &entries, Cursor { replica, seq: upto });
                if last.is_some_and(|last| upto >= last) {
                    last = None;
                    shared.heard(&peer.id);
                }
            }
            Reply::Refused { message } => bail!("the peer refused: {message}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Reply::Head` as peers from before `behind` have it.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum OldReply {
        Head { commit: Option<Oid> },
    }

    #[test]
    fn head_stays_readable_both_ways_within_the_alpn() {
        let new = rmp_serde::to_vec_named(&Reply::Head {
            commit: None,
            behind: Some(7),
        })
        .unwrap();
        assert_eq!(
            rmp_serde::from_slice::<OldReply>(&new).unwrap(),
            OldReply::Head { commit: None }
        );
        let old = rmp_serde::to_vec_named(&OldReply::Head { commit: None }).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Reply>(&old).unwrap(),
            Reply::Head {
                commit: None,
                behind: None
            }
        );
    }
}
