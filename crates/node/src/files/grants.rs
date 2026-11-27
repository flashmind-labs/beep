//! Who may fetch what from this node's blob store. A source grants one
//! peer one root while its offer is open; `transfer::gate` asks this table
//! about every connection and request, with what sync says of the peer, and
//! refuses the rest. A granted fetch counts the bytes it sent, for the offer
//! to show.

use crate::lock;
use iroh::EndpointId;
use iroh_blobs::{BlobFormat, HashAndFormat, protocol::GetRequest, provider::events::AbortReason};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Debug)]
pub(crate) struct Grants {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    next: u64,
    grants: HashMap<u64, Grant>,
    /// The peer of each connection the handler let in.
    connections: HashMap<u64, EndpointId>,
}

#[derive(Debug)]
struct Grant {
    peer: EndpointId,
    content: HashAndFormat,
    /// The most bytes one fetch of it has sent.
    sent: watch::Sender<u64>,
}

/// A grant, held while its offer is open.
#[derive(Debug)]
pub(crate) struct GrantGuard {
    grants: Arc<Grants>,
    id: u64,
    sent: watch::Receiver<u64>,
}

impl GrantGuard {
    /// The most bytes one fetch of the grant has sent so far.
    pub(crate) fn sent(&self) -> watch::Receiver<u64> {
        self.sent.clone()
    }
}

impl Drop for GrantGuard {
    fn drop(&mut self) {
        lock(&self.grants.inner).grants.remove(&self.id);
    }
}

impl Grants {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::default(),
        }
    }

    /// Lets `peer` fetch `content` until the guard drops.
    pub(crate) fn grant(self: &Arc<Self>, peer: EndpointId, content: HashAndFormat) -> GrantGuard {
        let mut inner = lock(&self.inner);
        let id = inner.next;
        inner.next += 1;
        let (sent, watched) = watch::channel(0);
        inner.grants.insert(
            id,
            Grant {
                peer,
                content,
                sent,
            },
        );
        GrantGuard {
            grants: self.clone(),
            id,
            sent: watched,
        }
    }

    /// Lets `peer` in on `connection` when it is `synced`, a peer of a synced
    /// project, or holds a grant.
    pub(crate) fn connect(
        &self,
        connection: u64,
        peer: Option<EndpointId>,
        synced: bool,
    ) -> Result<(), AbortReason> {
        let mut inner = lock(&self.inner);

        match peer {
            Some(peer) if synced || inner.grants.values().any(|grant| grant.peer == peer) => {
                inner.connections.insert(connection, peer);
                Ok(())
            }
            _ => Err(AbortReason::Permission),
        }
    }

    /// The peer `connection` let in.
    pub(crate) fn peer(&self, connection: u64) -> Option<EndpointId> {
        lock(&self.inner).connections.get(&connection).copied()
    }

    pub(crate) fn closed(&self, connection: u64) {
        lock(&self.inner).connections.remove(&connection);
    }

    /// Whether the peer on `connection` may make `request`: a root it was
    /// granted, and for a file only the blob itself, or, when `synced`, a
    /// blob of a project the peer syncs, which `synced` says of the
    /// peer and is asked only when no grant matches. A granted root comes with where
    /// its bytes sent go.
    pub(crate) fn get(
        &self,
        connection: u64,
        request: &GetRequest,
        synced: impl FnOnce(&EndpointId) -> bool,
    ) -> Result<Option<watch::Sender<u64>>, AbortReason> {
        let inner = lock(&self.inner);
        let peer = inner
            .connections
            .get(&connection)
            .ok_or(AbortReason::Permission)?;

        let granted = inner.grants.values().find(|grant| {
            grant.peer == *peer
                && grant.content.hash == request.hash
                && (grant.content.format == BlobFormat::HashSeq || request.ranges.is_blob())
        });
        match granted {
            Some(grant) => Ok(Some(grant.sent.clone())),
            None if request.ranges.is_blob() && synced(peer) => Ok(None),
            None => Err(AbortReason::Permission),
        }
    }
}

#[cfg(test)]
#[path = "../tests/files_grants.rs"]
mod tests;
