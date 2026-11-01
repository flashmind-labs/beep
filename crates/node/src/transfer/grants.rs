//! Who may fetch what from this node's blob store. A source grants one
//! peer one root while its offer is open; the blobs protocol asks the
//! handler here about every connection and request, and refuses the rest.

use iroh::EndpointId;
use iroh_blobs::{
    BlobFormat, Hash, HashAndFormat,
    protocol::GetRequest,
    provider::events::{
        AbortReason, ConnectMode, EventMask, EventSender, ObserveMode, ProviderMessage,
        RequestMode, ThrottleMode,
    },
};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
};
use tokio_util::task::AbortOnDropHandle;

/// How many provider events wait for the handler.
const EVENT_QUEUE: usize = 32;

/// Who may fetch beyond the open offers: the peers of a synced project,
/// for the files this node holds of it.
pub(crate) trait Access: Send + Sync {
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
}

/// A grant, held while its offer is open.
#[derive(Debug)]
pub(crate) struct GrantGuard {
    grants: Arc<Grants>,
    id: u64,
}

impl Drop for GrantGuard {
    fn drop(&mut self) {
        self.grants.lock().grants.remove(&self.id);
    }
}

impl Grants {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lets `peer` fetch `content` until the guard drops.
    pub(crate) fn grant(self: &Arc<Self>, peer: EndpointId, content: HashAndFormat) -> GrantGuard {
        let mut inner = self.lock();
        let id = inner.next;
        inner.next += 1;
        inner.grants.insert(id, Grant { peer, content });
        GrantGuard {
            grants: self.clone(),
            id,
        }
    }

    /// Lets `access` admit peers beside the open offers. Set once.
    pub(crate) fn set_access(&self, access: Arc<dyn Access>) {
        if self.access.set(access).is_err() {
            log::warn!("blob access was set twice; the first stays");
        }
    }

    fn connect(&self, connection: u64, peer: Option<EndpointId>) -> Result<(), AbortReason> {
        let synced = peer.is_some_and(|peer| {
            self.access
                .get()
                .is_some_and(|access| access.may_connect(&peer))
        });
        let mut inner = self.lock();
        match peer {
            Some(peer) if synced || inner.grants.values().any(|grant| grant.peer == peer) => {
                inner.connections.insert(connection, peer);
                Ok(())
            }
            _ => Err(AbortReason::Permission),
        }
    }

    fn closed(&self, connection: u64) {
        self.lock().connections.remove(&connection);
    }

    /// Whether the peer on `connection` may make `request`: a root it was
    /// granted, and for a file only the blob itself.
    fn get(&self, connection: u64, request: &GetRequest) -> Result<(), AbortReason> {
        let inner = self.lock();
        let peer = inner
            .connections
            .get(&connection)
            .ok_or(AbortReason::Permission)?;
        let allowed = inner.grants.values().any(|grant| {
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
        if allowed || synced() {
            Ok(())
        } else {
            Err(AbortReason::Permission)
        }
    }
}

/// The events the blobs protocol sends, answered from `grants` by a task
/// the handle owns. Requests that could reach other blobs (`get_many`,
/// `observe`) and writes (`push`) are refused.
pub(crate) fn events(grants: Arc<Grants>) -> (EventSender, AbortOnDropHandle<()>) {
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        get: RequestMode::Intercept,
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
                    msg.tx.send(answer).await.ok();
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

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn only_the_granted_peer_gets_the_granted_root() {
        let grants = Arc::new(Grants::default());
        let peer = SecretKey::generate().public();
        let stranger = SecretKey::generate().public();
        let file = Hash::new(b"file");
        let folder = Hash::new(b"folder");
        let guard = grants.grant(peer, HashAndFormat::raw(file));
        let folder_guard = grants.grant(peer, HashAndFormat::hash_seq(folder));

        assert!(grants.connect(1, Some(stranger)).is_err());
        assert!(grants.connect(2, None).is_err());
        assert!(grants.connect(3, Some(peer)).is_ok());
        assert!(grants.get(3, &GetRequest::blob(file)).is_ok());
        assert!(grants.get(3, &GetRequest::all(folder)).is_ok());
        // A file's hash read as a folder would reach other blobs.
        assert!(grants.get(3, &GetRequest::all(file)).is_err());
        assert!(
            grants
                .get(3, &GetRequest::blob(Hash::new(b"other")))
                .is_err()
        );
        assert!(grants.get(1, &GetRequest::blob(file)).is_err());

        drop(guard);
        assert!(grants.get(3, &GetRequest::blob(file)).is_err());
        drop(folder_guard);
        assert!(grants.connect(4, Some(peer)).is_err());
        grants.closed(3);
        assert!(grants.get(3, &GetRequest::all(folder)).is_err());
    }

    struct Member(EndpointId, Hash);

    impl Access for Member {
        fn may_connect(&self, peer: &EndpointId) -> bool {
            *peer == self.0
        }
        fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool {
            *peer == self.0 && *hash == self.1
        }
    }

    #[test]
    fn a_synced_peer_gets_only_the_project_blobs_and_only_raw() {
        let grants = Arc::new(Grants::default());
        let member = SecretKey::generate().public();
        let stranger = SecretKey::generate().public();
        let file = Hash::new(b"synced");
        grants.set_access(Arc::new(Member(member, file)));
        assert!(grants.connect(1, Some(stranger)).is_err());
        assert!(grants.connect(2, Some(member)).is_ok());
        assert!(grants.get(2, &GetRequest::blob(file)).is_ok());
        assert!(grants.get(2, &GetRequest::all(file)).is_err());
        assert!(
            grants
                .get(2, &GetRequest::blob(Hash::new(b"other")))
                .is_err()
        );
    }
}
