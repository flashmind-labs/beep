//! Who may fetch what from this node's blob store. A source grants one
//! peer one root while its offer is open; the blobs protocol asks the
//! handler here about every connection and request, and refuses the rest.
//! A granted fetch counts the bytes it sent, for the offer to show.

use crate::lock;
use iroh::EndpointId;
use iroh_blobs::{
    BlobFormat, Hash, HashAndFormat,
    protocol::GetRequest,
    provider::events::{
        AbortReason, ConnectMode, EventMask, EventSender, ObserveMode, ProviderMessage,
        RequestMode, RequestUpdate, ThrottleMode,
    },
};
use irpc::channel::mpsc::Receiver;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

/// How many provider events wait for the handler.
const EVENT_QUEUE: usize = 32;

/// Who may fetch beyond the open offers: the peers of a synced project,
/// for the files this node holds of it.
pub(crate) trait Access: Send + Sync {
    /// Whether `peer` may fetch anything at all: a device the node's mesh
    /// lists as removed may not, whatever is open for it. Others are left
    /// to the grants.
    fn allows(&self, _peer: &EndpointId) -> bool {
        true
    }
    fn may_connect(&self, peer: &EndpointId) -> bool;
    fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool;
}

#[derive(Default)]
pub(crate) struct Grants {
    inner: Mutex<Inner>,
    access: OnceLock<Arc<dyn Access>>,
}

impl fmt::Debug for Grants {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grants")
            .field("inner", &self.inner)
            .finish()
    }
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

    /// Lets `access` admit peers beside the open offers. Set once.
    pub(crate) fn set_access(&self, access: Arc<dyn Access>) {
        if self.access.set(access).is_err() {
            log::warn!("blob access was set twice; the first stays");
        }
    }

    fn connect(&self, connection: u64, peer: Option<EndpointId>) -> Result<(), AbortReason> {
        if peer.is_some_and(|peer| !self.allows(&peer)) {
            return Err(AbortReason::Permission);
        }

        let synced = peer.is_some_and(|peer| {
            self.access
                .get()
                .is_some_and(|access| access.may_connect(&peer))
        });
        let mut inner = lock(&self.inner);

        match peer {
            Some(peer) if synced || inner.grants.values().any(|grant| grant.peer == peer) => {
                inner.connections.insert(connection, peer);
                Ok(())
            }
            _ => Err(AbortReason::Permission),
        }
    }

    fn allows(&self, peer: &EndpointId) -> bool {
        self.access.get().is_none_or(|access| access.allows(peer))
    }

    fn closed(&self, connection: u64) {
        lock(&self.inner).connections.remove(&connection);
    }

    /// Whether the peer on `connection` may make `request`: a root it was
    /// granted, and for a file only the blob itself. A granted root comes
    /// with where its bytes sent go.
    fn get(
        &self,
        connection: u64,
        request: &GetRequest,
    ) -> Result<Option<watch::Sender<u64>>, AbortReason> {
        let inner = lock(&self.inner);
        let peer = inner
            .connections
            .get(&connection)
            .ok_or(AbortReason::Permission)?;
        if !self.allows(peer) {
            return Err(AbortReason::Permission);
        }

        let granted = inner.grants.values().find(|grant| {
            grant.peer == *peer
                && grant.content.hash == request.hash
                && (grant.content.format == BlobFormat::HashSeq || request.ranges.is_blob())
        });
        let synced = || {
            request.ranges.is_blob()
                && self
                    .access
                    .get()
                    .is_some_and(|access| access.may_get(peer, &request.hash))
        };
        match granted {
            Some(grant) => Ok(Some(grant.sent.clone())),
            None if synced() => Ok(None),
            None => Err(AbortReason::Permission),
        }
    }
}

/// The events the blobs protocol sends, answered from `grants` by a task
/// the handle owns. Requests that could reach other blobs (`get_many`,
/// `observe`) and writes (`push`) are refused.
pub(crate) fn events(grants: Arc<Grants>) -> (EventSender, AbortOnDropHandle<()>) {
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        get: RequestMode::InterceptLog,
        get_many: RequestMode::Disabled,
        push: RequestMode::Disabled,
        observe: ObserveMode::Intercept,
        throttle: ThrottleMode::None,
    };
    let (sender, mut events) = EventSender::channel(EVENT_QUEUE, mask);
    let task = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            // A failed answer means the request is gone already.
            match event {
                ProviderMessage::ClientConnected(msg) => {
                    let answer = grants.connect(msg.connection_id, msg.endpoint_id);

                    if answer.is_err() {
                        log::info!("refused a blobs connection from {:?}", msg.endpoint_id);
                    }

                    msg.tx.send(answer).await.ok();
                }
                ProviderMessage::ConnectionClosed(msg) => grants.closed(msg.connection_id),
                ProviderMessage::GetRequestReceived(msg) => {
                    let answer = grants.get(msg.connection_id, &msg.request);
                    let sent = answer.as_ref().ok().cloned().flatten();
                    msg.tx.send(answer.map(drop)).await.ok();
                    // The fetch waits on its updates being read, so they
                    // are read to the end even when nobody counts them.
                    tokio::spawn(count(msg.rx, sent));
                }
                ProviderMessage::ObserveRequestReceived(msg) => {
                    msg.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                ProviderMessage::GetManyRequestReceived(msg) => {
                    msg.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                ProviderMessage::PushRequestReceived(msg) => {
                    msg.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                // Notifications of the modes left at none never come.
                _ => {}
            }
        }
    });
    (sender, AbortOnDropHandle::new(task))
}

/// Reads one fetch's updates to their end, counting the bytes it sent of
/// the blobs it started into `sent`, which keeps the most any fetch sent.
async fn count(mut updates: Receiver<RequestUpdate>, sent: Option<watch::Sender<u64>>) {
    // The blobs this fetch finished, and the one it sends now.
    let (mut finished, mut size, mut offset) = (0u64, 0u64, 0u64);

    while let Ok(Some(update)) = updates.recv().await {
        match update {
            RequestUpdate::Started(started) => {
                finished = finished.saturating_add(size);
                (size, offset) = (started.size, 0);
            }
            RequestUpdate::Progress(progress) => offset = progress.end_offset,
            RequestUpdate::Completed(_) | RequestUpdate::Aborted(_) => {}
        }

        if let Some(sent) = &sent {
            let now = finished.saturating_add(offset.min(size));
            sent.send_if_modified(|most| {
                let more = now > *most;
                *most = (*most).max(now);
                more
            });
        }
    }
}

#[cfg(test)]
#[path = "../tests/files_grants.rs"]
mod tests;
