//! `Mesh`: one device's membership. An owner task keeps a dialer per other
//! member; `link` runs each connection; `join` handles tickets.

mod channels;
mod lifecycle;
mod membership;
mod owner;
mod protocol;
pub mod values;

use owner::own;
pub use protocol::{JoinProtocol, MeshProtocol};

use crate::state::now_ms;
pub(crate) use crate::store::AddrNote;
use crate::store::{Core, SharedView, Store, View};
use crate::wire;
use crate::{Device, DeviceKind, Key, MeshError};
use iroh::endpoint::{
    Connection, IdleTimeout, QuicTransportConfig, RecvStream, SendStream, VarInt,
};
use iroh::protocol::{IncomingFilterOutcome, Router, RouterBuilder};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};
use tokio::sync::{Notify, Semaphore, broadcast, watch};
use tokio::task::{AbortHandle, JoinHandle, spawn_blocking};
use tracing::{debug, error, warn};

/// How long a ticket lives unless the caller says otherwise.
pub const DEFAULT_TICKET_TTL: Duration = Duration::from_secs(60 * 60);
pub const PEER_ALPN: &[u8] = b"beep-peer/2";
pub const JOIN_ALPN: &[u8] = b"beep-join/2";

const CHANGES: usize = 256;
/// How far ahead of our clock a relayed address counter may be.
const MAX_AHEAD_MS: u64 = 60_000;
/// iroh already pings each path every 5 s, which keeps a link up on its own;
/// this ping only covers a connection whose paths stopped pinging.
const KEEP_ALIVE: Duration = Duration::from_secs(10);
/// A link with no packets this long is dead. QUIC takes the smaller of the
/// two peers' values, so a link to an older peer still ends after 10 s.
/// It matches iroh's idle limit for relay paths, so a link rides out a
/// network handoff instead of redialing and syncing again.
const IDLE_MS: u32 = 30_000;
/// Further streams one peer may have open at once.
pub(crate) const MAX_STREAMS: usize = 64;
/// Times `open_bi` goes on to a newer connection.
const OPEN_RETRIES: u32 = 2;
/// Addresses taken from one message.
const MAX_ADDRS: usize = 256;

/// A bi stream a peer opened after the sync stream. The first bytes on it
/// are the caller's own protocol.
pub struct PeerStream {
    pub peer: EndpointId,
    /// What the peer is, from the signed device list.
    pub kind: DeviceKind,
    pub send: SendStream,
    pub recv: RecvStream,
}

/// The transport settings a mesh endpoint should be built with: idle links
/// are pinged and a dead one is given up on after thirty seconds. Datagrams go
/// singly, as `beep_proto::port::transport` has it, because sync shares the
/// endpoint.
pub fn transport_config() -> QuicTransportConfig {
    QuicTransportConfig::builder()
        .enable_segmentation_offload(false)
        .keep_alive_interval(KEEP_ALIVE)
        .max_idle_timeout(Some(IdleTimeout::from(VarInt::from_u32(IDLE_MS))))
        .build()
}

/// A router for a mesh endpoint. A connection whose first packet does not
/// decrypt is dropped as iroh would drop it, but logged with where it came
/// from: no endpoint id is proven before the handshake, and only a relay
/// names one.
pub fn router(endpoint: &Endpoint) -> RouterBuilder {
    Router::builder(endpoint.clone()).incoming_filter(Arc::new(|incoming| {
        if incoming.decrypt().is_some() {
            return IncomingFilterOutcome::Accept;
        }

        warn!(remote = ?incoming.remote_addr(), "ignoring a connection whose first packet does not decrypt");
        IncomingFilterOutcome::Ignore
    }))
}

pub type StreamHandler =
    Arc<dyn Fn(PeerStream) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct DeviceStatus {
    pub device: Device,
    pub online: bool,
    pub addr: Option<EndpointAddr>,
    /// When its link here last ended, since this mesh opened; `None` while
    /// online or when it was not reached since.
    pub last_seen: Option<SystemTime>,
}

/// The ways to reach `addr`, for a log or a report: its relay and its
/// addresses, or that there are none and it is looked up by its id.
pub fn ways(addr: &EndpointAddr) -> String {
    let relays: Vec<_> = addr.relay_urls().map(ToString::to_string).collect();
    let ips: Vec<_> = addr.ip_addrs().map(ToString::to_string).collect();

    if relays.is_empty() && ips.is_empty() {
        return "its id alone".to_owned();
    }

    let relay = if relays.is_empty() {
        "no relay".to_owned()
    } else {
        format!("relay {}", relays.join(" "))
    };

    if ips.is_empty() {
        return format!("{relay}, no address");
    }

    format!("{relay}, {}", ips.join(" "))
}

pub(crate) struct Live {
    pub conn: Connection,
    /// Who opened the connection: the smaller id's wins when both did.
    pub dialer: EndpointId,
    /// The peer's `session` from its hello.
    pub session: u64,
    /// Rung when a connection of the same instance won the tie: this one
    /// leaves the mesh link and stays only for the streams still on it.
    pub retire: Arc<Notify>,
}

pub(crate) struct Retired {
    pub conn: Connection,
    pub session: u64,
}

pub(crate) struct Inner {
    pub endpoint: Endpoint,
    /// Random per start: peers tell a restart from a second dial by it.
    pub session: u64,
    pub store: Store,
    view: SharedView,
    pub rev: Arc<watch::Sender<u64>>,
    pub changes: broadcast::Sender<Key>,
    conns: Mutex<BTreeMap<EndpointId, Live>>,
    channels: channels::Channels,
    /// Connections that left the mesh link and stay for their open streams,
    /// by stable id. A removal cuts these too.
    retired: Mutex<BTreeMap<usize, Retired>>,
    addrs: Mutex<BTreeMap<EndpointId, AddrNote>>,
    /// When each peer's link last ended, since the mesh opened.
    last_seen: Mutex<BTreeMap<EndpointId, SystemTime>>,
    /// Our own address and its counter.
    own_addr: Mutex<AddrNote>,
    /// Rung when any address changes, for dialers waiting to try again.
    pub addr_wake: Notify,
    /// Rung when a connection, an address or the device list changes.
    pub wake: Notify,
    pub handler: Mutex<Option<StreamHandler>>,
    pub joins: Semaphore,
    pub closed: AtomicBool,
}

impl Inner {
    pub fn view(&self) -> Arc<View> {
        self.view
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn me(&self) -> EndpointId {
        self.endpoint.id()
    }

    pub fn is_member(&self, id: EndpointId) -> bool {
        self.view().devices.is_member(id)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    pub fn conns(&self) -> MutexGuard<'_, BTreeMap<EndpointId, Live>> {
        self.conns.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn retired(&self) -> MutexGuard<'_, BTreeMap<usize, Retired>> {
        self.retired.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn last_seen(&self) -> MutexGuard<'_, BTreeMap<EndpointId, SystemTime>> {
        self.last_seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn addrs(&self) -> MutexGuard<'_, BTreeMap<EndpointId, AddrNote>> {
        self.addrs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The connection to `id` if it is up.
    pub fn live(&self, id: EndpointId) -> Option<Connection> {
        let conns = self.conns();
        let live = conns.get(&id)?;
        live.conn
            .close_reason()
            .is_none()
            .then(|| live.conn.clone())
    }

    /// Addresses to hand a peer: ours, with a counter that rises when it
    /// changes, and the others with the counters their owners gave them.
    pub fn addr_list(&self) -> Vec<AddrNote> {
        let current = self.endpoint.addr();
        let own = {
            let mut own = self.own_addr.lock().unwrap_or_else(PoisonError::into_inner);
            if own.addr != current {
                own.generation = own.generation.saturating_add(1).max(now_ms());
                own.addr = current;
            }
            own.clone()
        };
        let mut list: Vec<_> = self.addrs().values().cloned().collect();
        list.push(own);
        list.truncate(MAX_ADDRS);
        list
    }

    /// Remembers where members were last reached. `from` is the peer whose
    /// authenticated link carried the notes. Its own note replaces ours
    /// whatever its counter. A relayed note needs a higher counter than what
    /// we hold, so peers holding different notes for a third settle on the
    /// newest, and one stamped more than a minute ahead of our clock is
    /// dropped, so no relay can pin a victim's address.
    pub async fn learn(&self, list: Vec<AddrNote>, from: EndpointId) {
        let ceiling = now_ms().saturating_add(MAX_AHEAD_MS);
        let view = self.view();
        let mut changed = false;
        {
            let mut known = self.addrs();

            for note in list.into_iter().take(MAX_ADDRS) {
                let id = note.addr.id;

                if id == self.me() || !view.devices.is_member(id) {
                    continue;
                }

                let first_hand = from == id;

                if !first_hand && note.generation > ceiling {
                    continue;
                }

                let newer = known
                    .get(&id)
                    .is_none_or(|old| note.generation > old.generation);
                let differs = known.get(&id).is_none_or(|old| old.addr != note.addr);
                if (first_hand && differs) || (!first_hand && newer) {
                    known.insert(id, note);
                    changed = true;
                }
            }
        }
        if changed {
            let all: Vec<_> = self.addrs().values().cloned().collect();

            if let Err(e) = self.store.run(move |core| core.save_addrs(&all)).await {
                debug!("could not save peer addresses: {e}");
            }

            self.addr_wake.notify_waiters();
            self.bump();
        }
    }

    pub fn bump(&self) {
        self.rev.send_modify(|n| *n = n.wrapping_add(1));
        self.wake.notify_waiters();
    }
}

struct Guard {
    /// How many `Handle`s to this guard are alive.
    handles: watch::Sender<usize>,
    inner: Arc<Inner>,
    owner: AbortHandle,
    _supervisor: JoinHandle<()>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Relaxed);
        self.owner.abort();

        for live in self.inner.conns().values() {
            live.conn.close(0u32.into(), b"closed");
        }

        for conn in self.inner.channel_connections() {
            conn.close(0u32.into(), b"closed");
        }

        for retired in self.inner.retired().values() {
            retired.conn.close(0u32.into(), b"closed");
        }
    }
}

/// A `Mesh`'s share of its guard, counted so `Mesh::released` can say when
/// the others are gone.
struct Handle(Arc<Guard>);

impl Clone for Handle {
    fn clone(&self) -> Self {
        self.0.handles.send_modify(|handles| *handles += 1);
        Self(self.0.clone())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.handles.send_modify(|handles| *handles -= 1);
    }
}

/// One device's membership in a mesh. Cheap to clone; the last clone to
/// drop stops the background tasks and closes every connection.
#[derive(Clone)]
pub struct Mesh {
    inner: Arc<Inner>,
    _guard: Handle,
}

impl Debug for Mesh {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mesh")
            .field("me", &self.inner.me())
            .finish()
    }
}

impl Mesh {
    fn run(endpoint: Endpoint, core: Core, addrs: Vec<AddrNote>) -> Result<Mesh, MeshError> {
        let me = endpoint.id();
        let view = core.view();
        let rev = core.rev();
        let mut known: BTreeMap<EndpointId, AddrNote> = BTreeMap::new();

        for note in addrs.into_iter().filter(|n| n.addr.id != me) {
            let id = note.addr.id;

            if known
                .get(&id)
                .is_none_or(|old| note.generation > old.generation)
            {
                known.insert(id, note);
            }
        }

        let first = AddrNote {
            addr: endpoint.addr(),
            generation: now_ms(),
        };
        let inner = Arc::new(Inner {
            endpoint,
            session: fastrand::u64(..),
            store: Store::spawn(core)?,
            view,
            rev,
            changes: broadcast::channel(CHANGES).0,
            conns: Mutex::default(),
            channels: Mutex::default(),
            retired: Mutex::default(),
            addrs: Mutex::new(known),
            last_seen: Mutex::default(),
            own_addr: Mutex::new(first),
            addr_wake: Notify::new(),
            wake: Notify::new(),
            handler: Mutex::new(None),
            joins: Semaphore::new(8),
            closed: AtomicBool::new(false),
        });
        let owner = tokio::spawn(own(inner.clone()));
        let stop = owner.abort_handle();
        // The owner's handle lives in the supervisor, which says so if the
        // owner panics instead of letting the mesh go quiet.
        let supervisor = tokio::spawn(async move {
            if let Err(e) = owner.await
                && e.is_panic()
            {
                error!("the mesh owner task panicked: {e}");
            }
        });
        Ok(Mesh {
            _guard: Handle(Arc::new(Guard {
                handles: watch::Sender::new(1),
                inner: inner.clone(),
                owner: stop,
                _supervisor: supervisor,
            })),
            inner,
        })
    }

    /// What peers tell a restart of this mesh from a second dial by.
    #[cfg(test)]
    pub(crate) fn session(&self) -> u64 {
        self.inner.session
    }

    pub fn peer(&self, id: EndpointId) -> Option<Connection> {
        self.inner.live(id)
    }

    /// Opens a bi stream to `peer` on the current connection. When that
    /// connection is closed because a newer link replaced it (two members
    /// dialing at once keep one), the newer one is tried, twice at most.
    pub async fn open_bi(&self, peer: EndpointId) -> Result<(SendStream, RecvStream), MeshError> {
        let first = self
            .inner
            .live(peer)
            .ok_or_else(|| wire::net("not connected"))?;
        self.open_from(peer, first).await
    }

    pub(crate) async fn open_from(
        &self,
        peer: EndpointId,
        first: Connection,
    ) -> Result<(SendStream, RecvStream), MeshError> {
        let mut conn = first;
        let mut retries = OPEN_RETRIES;

        loop {
            match conn.open_bi().await {
                Ok(streams) => return Ok(streams),
                Err(e) => {
                    let newer = self
                        .inner
                        .live(peer)
                        .filter(|live| live.stable_id() != conn.stable_id());
                    match newer {
                        Some(live) if retries > 0 => {
                            retries -= 1;
                            conn = live;
                        }
                        _ => return Err(wire::net(e)),
                    }
                }
            }
        }
    }

    pub fn protocols(&self) -> (MeshProtocol, JoinProtocol) {
        (
            MeshProtocol {
                inner: self.inner.clone(),
            },
            JoinProtocol {
                inner: self.inner.clone(),
            },
        )
    }
}

async fn blocking<R: Send + 'static>(
    f: impl FnOnce() -> Result<R, MeshError> + Send + 'static,
) -> Result<R, MeshError> {
    spawn_blocking(f).await.map_err(|_| MeshError::Closed)?
}

/// `close` found another handle to the mesh alive. The mesh is handed back,
/// still running.
pub struct InUse(pub Mesh);

impl Debug for InUse {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("InUse")
    }
}

impl fmt::Display for InUse {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("the mesh is still in use")
    }
}
