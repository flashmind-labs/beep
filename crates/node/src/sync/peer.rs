//! Node to node: each node pulls every project's list from each of its
//! peers on `ALPN`, directly over iroh, so sync goes on while the server
//! is down. A pull names the peer's replica and the last change it has
//! from it; the peer sends what changed since, then each change as it
//! happens. Only a project's peers may pull it.
//!
//! Messages are msgpack maps with named fields, like the node link, so
//! nodes of different versions can add fields.

use super::{
    Syncs,
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
    time::{sleep, timeout},
};

pub(crate) const ALPN: &[u8] = b"beep-sync/1";

/// The most entries one batch carries.
const BATCH: usize = 1000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);
/// The most projects one peer pulls at once over a connection.
const MAX_STREAMS: usize = 64;

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
                Some(ended) = streams.join_next() => match ended {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::debug!("a pull from {peer} ended: {e:#}"),
                    Err(e) => log::warn!("a pull task failed: {e}"),
                },
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
        .filter(|shared| shared.has_peer(&peer))
    else {
        let message = format!("{project} is not synced with this node");
        return tx.send(&Reply::Refused { message }).await;
    };
    let mut seq = shared.subscribe();
    let mut since = if replica == shared.replica() {
        since
    } else {
        0
    };
    loop {
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
            pulled = pull_once(&shared, &endpoint, &peer) => pulled,
            () = shared.stopped() => return,
        };
        match pulled {
            Ok(()) => wait = FIRST_RETRY,
            Err(e) => log::debug!("pulling {} from {}: {e:#}", shared.name(), peer.id),
        }
        tokio::select! {
            () = sleep(wait) => {}
            () = shared.stopped() => return,
        }
        wait = (wait * 2).min(LAST_RETRY);
    }
}

async fn pull_once(shared: &Shared, endpoint: &Endpoint, peer: &EndpointAddr) -> Result<()> {
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
    while let Some(reply) = rx.recv::<Reply>().await? {
        match reply {
            Reply::Changes {
                replica,
                entries,
                upto,
            } => shared.merge_from(&peer.id, &entries, Cursor { replica, seq: upto }),
            Reply::Refused { message } => bail!("the peer refused: {message}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let pull = Request::Pull {
            project: "game".into(),
            replica: 7,
            since: 3,
        };
        let bytes = rmp_serde::to_vec_named(&pull).unwrap();
        assert_eq!(rmp_serde::from_slice::<Request>(&bytes).unwrap(), pull);
        let changes = Reply::Changes {
            replica: 7,
            entries: Vec::new(),
            upto: 9,
        };
        let bytes = rmp_serde::to_vec_named(&changes).unwrap();
        assert_eq!(rmp_serde::from_slice::<Reply>(&bytes).unwrap(), changes);
    }
}
