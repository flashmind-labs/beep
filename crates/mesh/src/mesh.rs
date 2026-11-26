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

use crate::inner::Inner;
pub use crate::inner::PeerStream;
use crate::store::{AddrNote, Core};
use crate::wire;
use crate::{Device, MeshError};
use iroh::endpoint::{
    Connection, IdleTimeout, QuicTransportConfig, RecvStream, SendStream, VarInt,
};
use iroh::protocol::{IncomingFilterOutcome, Router, RouterBuilder};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinHandle, spawn_blocking};
use tracing::{error, warn};

/// How long a ticket lives unless the caller says otherwise.
pub const DEFAULT_TICKET_TTL: Duration = Duration::from_secs(60 * 60);
pub const PEER_ALPN: &[u8] = b"beep-peer/2";
pub const JOIN_ALPN: &[u8] = b"beep-join/2";

/// iroh already pings each path every 5 s, which keeps a link up on its own;
/// this ping only covers a connection whose paths stopped pinging.
const KEEP_ALIVE: Duration = Duration::from_secs(10);
/// A link with no packets this long is dead. QUIC takes the smaller of the
/// two peers' values, so a link to an older peer still ends after 10 s.
/// It matches iroh's idle limit for relay paths, so a link rides out a
/// network handoff instead of redialing and syncing again.
const IDLE_MS: u32 = 30_000;
/// Times `open_bi` goes on to a newer connection.
const OPEN_RETRIES: u32 = 2;
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
        let inner = Inner::open(endpoint, core, addrs)?;
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

    /// The streams peers open after the sync stream, whose first bytes are the
    /// caller's own protocol. One reader takes them: a call replaces the
    /// reader before it. A stream is refused while nobody reads or the queue
    /// is full.
    pub fn incoming(&self) -> mpsc::Receiver<PeerStream> {
        self.inner.take_incoming()
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
