//! `Mesh`: one device's membership. An owner task keeps a dialer per other
//! member; `link` runs each connection; `join` handles tickets.

use crate::frame;
use crate::state::now_ms;
use crate::store::{Core, SharedView, Store, View};
use crate::{Device, DeviceKind, JoinTicket, Key, MeshError, link};
use iroh::endpoint::{
    Connection, IdleTimeout, QuicTransportConfig, RecvStream, SendStream, VarInt,
};
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore, broadcast, watch};
use tokio::task::{AbortHandle, JoinHandle, JoinSet, spawn_blocking};
use tokio::time::{Instant, sleep, timeout};
use tracing::{debug, error, warn};

/// How long a ticket lives unless the caller says otherwise.
pub const DEFAULT_TICKET_TTL: Duration = Duration::from_secs(60 * 60);
pub const PEER_ALPN: &[u8] = b"beep-peer/1";
pub const JOIN_ALPN: &[u8] = b"beep-join/1";

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(12);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
/// A connection that lasted this long counts as having worked.
const STABLE: Duration = Duration::from_secs(5);
const CHANGES: usize = 256;
/// A removed peer's link may take this long to deliver the news before it
/// is cut.
const GRACE: Duration = Duration::from_secs(3);
/// The larger id lets the smaller dial first, so one dial does the work.
const HEAD_START: Duration = Duration::from_millis(300);
/// How far ahead of our clock a relayed address counter may be.
const MAX_AHEAD_MS: u64 = 60_000;
const KEEP_ALIVE: Duration = Duration::from_secs(3);
const IDLE_MS: u32 = 10_000;
/// Further streams one peer may have open at once.
pub(crate) const MAX_STREAMS: usize = 64;
/// Times `open_bi` goes on to a newer connection.
const OPEN_RETRIES: u32 = 2;
/// Addresses taken from one message.
const MAX_ADDRS: usize = 256;

/// Matches `crates/node/src/link/reconnect.rs`: 1 s doubling to 12 s, each
/// shifted by up to a quarter.
struct Backoff(Duration);

impl Backoff {
    fn pause(&mut self) -> Duration {
        let wait = self.0.mul_f64(0.75 + fastrand::f64() / 2.0);
        self.0 = (self.0 * 2).min(LAST_RETRY);
        wait
    }

    fn reset(&mut self) {
        self.0 = FIRST_RETRY;
    }
}

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
/// are pinged and a dead one is given up on after ten seconds. Datagrams go
/// singly, as `beep_proto::port::transport` has it, because sync shares the
/// endpoint.
pub fn transport_config() -> QuicTransportConfig {
    QuicTransportConfig::builder()
        .enable_segmentation_offload(false)
        .keep_alive_interval(KEEP_ALIVE)
        .max_idle_timeout(Some(IdleTimeout::from(VarInt::from_u32(IDLE_MS))))
        .build()
}

pub type StreamHandler =
    Arc<dyn Fn(PeerStream) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// An address and the owner's counter for it. Higher replaces lower.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AddrNote {
    pub addr: EndpointAddr,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub struct DeviceStatus {
    pub device: Device,
    pub online: bool,
    pub addr: Option<EndpointAddr>,
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

pub(crate) struct Inner {
    pub endpoint: Endpoint,
    pub me: EndpointId,
    /// Random per start: peers tell a restart from a second dial by it.
    pub session: u64,
    pub store: Store,
    view: SharedView,
    pub rev: Arc<watch::Sender<u64>>,
    pub changes: broadcast::Sender<Key>,
    pub conns: Mutex<BTreeMap<EndpointId, Live>>,
    /// Connections that left the mesh link and stay for their open streams,
    /// by stable id. A removal cuts these too.
    retired: Mutex<BTreeMap<usize, Connection>>,
    pub addrs: Mutex<BTreeMap<EndpointId, AddrNote>>,
    pub addr_updates: AtomicU64,
    /// Our own address and its counter.
    own_addr: Mutex<AddrNote>,
    /// Rung when any address changes, for dialers waiting out a backoff.
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

    pub fn is_member(&self, id: EndpointId) -> bool {
        self.view().devices.is_member(id)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    pub fn conns(&self) -> MutexGuard<'_, BTreeMap<EndpointId, Live>> {
        self.conns.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn retired(&self) -> MutexGuard<'_, BTreeMap<usize, Connection>> {
        self.retired.lock().unwrap_or_else(PoisonError::into_inner)
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
    pub async fn learn(&self, list: Vec<AddrNote>, from: Option<EndpointId>) {
        let ceiling = now_ms().saturating_add(MAX_AHEAD_MS);
        let view = self.view();
        let mut changed = false;
        {
            let mut known = self.addrs();
            for note in list.into_iter().take(MAX_ADDRS) {
                let id = note.addr.id;
                if id == self.me || !view.devices.is_member(id) {
                    continue;
                }
                let first_hand = from == Some(id);
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
            self.addr_updates.fetch_add(1, Ordering::Relaxed);
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
        for conn in self.inner.retired().values() {
            conn.close(0u32.into(), b"closed");
        }
    }
}

/// One device's membership in a mesh. Cheap to clone; the last clone to
/// drop stops the background tasks and closes every connection.
#[derive(Clone)]
pub struct Mesh {
    inner: Arc<Inner>,
    _guard: Arc<Guard>,
}

impl Debug for Mesh {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mesh").field("me", &self.inner.me).finish()
    }
}

impl Mesh {
    /// The mesh in `dir`, or `None` when this directory is not in one.
    pub async fn open(dir: &Path, endpoint: Endpoint) -> Result<Option<Mesh>, MeshError> {
        let dir = dir.to_owned();
        let key = endpoint.secret_key().clone();
        let loaded = {
            let dir = dir.clone();
            blocking(move || {
                Core::recover(&dir)?;
                let Some(devices) = Core::load(&dir)? else {
                    return Ok(None);
                };
                Ok(Some((
                    Core::start(&dir, key, devices)?,
                    Core::load_addrs(&dir),
                )))
            })
            .await?
        };
        match loaded {
            None => Ok(None),
            Some((core, addrs)) => Ok(Some(Self::run(endpoint, core, addrs)?)),
        }
    }

    /// Starts a mesh with this device, a desktop, as its only member.
    pub async fn create(dir: &Path, endpoint: Endpoint, name: &str) -> Result<Mesh, MeshError> {
        let (dir, name) = (dir.to_owned(), name.to_owned());
        let key = endpoint.secret_key().clone();
        let core = {
            let dir = dir.clone();
            blocking(move || {
                if Core::exists(&dir) {
                    return Err(MeshError::AlreadyMember);
                }
                let devices = crate::Devices::genesis(&key, &name, DeviceKind::Desktop);
                Core::start(&dir, key, devices)
            })
            .await?
        };
        Self::run(endpoint, core, Vec::new())
    }

    /// Joins the mesh `ticket` names. The kind comes from the desktop's
    /// signed `Add`, never from this side.
    pub async fn join(
        dir: &Path,
        endpoint: Endpoint,
        ticket: &JoinTicket,
        name: &str,
    ) -> Result<Mesh, MeshError> {
        let dir = dir.to_owned();
        if Core::exists(&dir) {
            return Err(MeshError::AlreadyMember);
        }
        let welcome = crate::join::redeem(&endpoint, ticket, name).await?;
        let key = endpoint.secret_key().clone();
        let mut addrs = welcome.addrs;
        addrs.push(AddrNote {
            addr: ticket.addr.clone(),
            generation: 0,
        });
        let core = {
            let (dir, addrs) = (dir.clone(), addrs.clone());
            blocking(move || {
                let core = Core::start(&dir, key, welcome.devices)?;
                core.save_addrs(&addrs)?;
                Ok(core)
            })
            .await?
        };
        Self::run(endpoint, core, addrs)
    }

    /// Deletes the mesh in `dir`, so the device is in none. `close` the `Mesh`
    /// first.
    pub async fn forget(dir: &Path) -> Result<(), MeshError> {
        let dir = dir.to_owned();
        blocking(move || Core::forget(&dir)).await
    }

    /// Stops this mesh and waits until its files are no longer written, so
    /// another can take the directory. It must be the only handle: with
    /// another clone alive the mesh is handed back in `InUse`, still running.
    pub async fn close(self) -> Result<(), InUse> {
        if Arc::strong_count(&self._guard) > 1 {
            return Err(InUse(self));
        }
        let inner = self.inner.clone();
        drop(self);
        inner.store.close().await;
        Ok(())
    }

    /// Joins the mesh `ticket` names in place of the one in `dir`, which
    /// must hold no device but this one, or have removed this one. `close` the old `Mesh` first. Nothing
    /// changes on disk until the ticket is redeemed (a ticket is spent by
    /// then: a crash after that needs a fresh one). The swap moves the old
    /// files aside and removes them once the new ones are on disk, and a
    /// crash during it leaves the old mesh or the new: `open` finishes or
    /// undoes it through `recover`. What the old mesh held is gone; read it
    /// first to carry it over.
    pub async fn join_replacing_lone(
        dir: &Path,
        endpoint: Endpoint,
        ticket: &JoinTicket,
        name: &str,
    ) -> Result<Mesh, MeshError> {
        let dir = dir.to_owned();
        let me = endpoint.id();
        {
            let dir = dir.clone();
            blocking(move || {
                Core::recover(&dir)?;
                match Core::load(&dir)? {
                    // A device that was removed has nothing to keep.
                    Some(devices)
                        if devices.is_member(me)
                            && devices.members().iter().any(|d| d.id != me) =>
                    {
                        Err(MeshError::NotLone)
                    }
                    _ => Ok(()),
                }
            })
            .await?;
        }
        let welcome = crate::join::redeem(&endpoint, ticket, name).await?;
        let key = endpoint.secret_key().clone();
        let mut addrs = welcome.addrs;
        addrs.push(AddrNote {
            addr: ticket.addr.clone(),
            generation: 0,
        });
        let core = {
            let (dir, addrs) = (dir.clone(), addrs.clone());
            blocking(move || Core::swap_in(&dir, key, welcome.devices, &addrs)).await?
        };
        Self::run(endpoint, core, addrs)
    }

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
            me,
            session: fastrand::u64(..),
            store: Store::spawn(core)?,
            view,
            rev,
            changes: broadcast::channel(CHANGES).0,
            conns: Mutex::default(),
            retired: Mutex::default(),
            addrs: Mutex::new(known),
            addr_updates: AtomicU64::new(0),
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
            _guard: Arc::new(Guard {
                inner: inner.clone(),
                owner: stop,
                _supervisor: supervisor,
            }),
            inner,
        })
    }

    /// Mints a single-use ticket. A node cannot.
    pub async fn invite(&self, kind: DeviceKind, ttl: Duration) -> Result<JoinTicket, MeshError> {
        let addr = self.inner.endpoint.addr();
        self.inner
            .store
            .run(move |core| core.invite(addr, kind, ttl))
            .await
    }

    /// Removes a device for good. A node cannot.
    pub async fn remove(&self, id: EndpointId) -> Result<(), MeshError> {
        self.inner.store.run(move |core| core.remove(id)).await
    }

    /// Whether `id` was removed from the mesh, for good.
    pub fn is_removed(&self, id: EndpointId) -> bool {
        self.inner.view().devices.is_removed(id)
    }

    /// This device's id.
    pub fn me(&self) -> EndpointId {
        self.inner.me
    }

    /// Whether `id` is a device of the mesh now. Asked per connection and
    /// per request, so a removed device is out at once.
    pub fn is_member(&self, id: EndpointId) -> bool {
        self.inner.is_member(id)
    }

    pub fn devices(&self) -> Vec<DeviceStatus> {
        let view = self.inner.view();
        let conns = self.inner.conns();
        let addrs = self.inner.addrs();
        view.devices
            .members()
            .into_iter()
            .map(|device| {
                let me = device.id == self.inner.me;
                DeviceStatus {
                    online: me
                        || conns
                            .get(&device.id)
                            .is_some_and(|l| l.conn.close_reason().is_none()),
                    addr: if me {
                        Some(self.inner.endpoint.addr())
                    } else {
                        addrs.get(&device.id).map(|n| n.addr.clone())
                    },
                    device,
                }
            })
            .collect()
    }

    /// Writes a value, durable on return. Desktops only.
    pub async fn set(&self, key: Key, value: Option<Vec<u8>>) -> Result<(), MeshError> {
        let touched = key.clone();
        self.inner
            .store
            .run(move |core| core.set(key, value))
            .await?;
        // No receiver is a normal state.
        self.inner.changes.send(touched).ok();
        Ok(())
    }

    /// Writes `value` only if this mesh never held `key`: a key it holds, or held and deleted,
    /// keeps its state. Returns whether it wrote. Desktops only.
    pub async fn set_if_new(&self, key: Key, value: Vec<u8>) -> Result<bool, MeshError> {
        if self.inner.view().held.contains(&key) {
            return Ok(false);
        }
        self.set(key, Some(value)).await?;
        Ok(true)
    }

    pub fn get(&self, key: &Key) -> Option<Vec<u8>> {
        self.inner.view().values.get(key).cloned()
    }

    /// Live keys whose text form starts with `prefix`, such as `project/`.
    pub fn scan(&self, prefix: &str) -> Vec<(Key, Vec<u8>)> {
        self.inner
            .view()
            .values
            .iter()
            .filter(|(key, _)| key.to_string().starts_with(prefix))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Whether any key whose text form starts with `prefix` was ever
    /// written, even if deleted since.
    pub fn has_held(&self, prefix: &str) -> bool {
        self.inner
            .view()
            .held
            .iter()
            .any(|key| key.to_string().starts_with(prefix))
    }

    /// Keys that changed, written here or taken from a peer. A receiver that
    /// falls behind gets `Lagged` and must rescan; subscribe before the first
    /// `scan` so nothing falls between the two.
    pub fn changes(&self) -> broadcast::Receiver<Key> {
        self.inner.changes.subscribe()
    }

    /// Rises whenever the device list, the state or who is online changes.
    pub fn updates(&self) -> watch::Receiver<u64> {
        self.inner.rev.subscribe()
    }

    /// Feeds an address for a member that did not come from the peer
    /// itself, such as one found on the local network. Higher `generation`
    /// wins.
    pub async fn hint_addr(&self, addr: EndpointAddr, generation: u64) {
        self.inner
            .learn(vec![AddrNote { addr, generation }], None)
            .await;
    }

    /// How many times the address table has changed.
    pub fn addr_updates(&self) -> u64 {
        self.inner.addr_updates.load(Ordering::Relaxed)
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
            .ok_or_else(|| frame::net("not connected"))?;
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
                        _ => return Err(frame::net(e)),
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

/// Serves `beep-peer/1`: members only.
#[derive(Clone)]
pub struct MeshProtocol {
    inner: Arc<Inner>,
}

impl MeshProtocol {
    /// Sets what serves each bi stream a peer opens after the sync stream.
    pub fn on_stream(&self, handler: StreamHandler) {
        *self
            .inner
            .handler
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(handler);
    }
}

impl Debug for MeshProtocol {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("MeshProtocol")
    }
}

impl ProtocolHandler for MeshProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let peer = connection.remote_id();
        if self.inner.is_closed() {
            connection.close(1u32.into(), b"closed");
            return Ok(());
        }
        if !self.inner.is_member(peer) {
            if self.inner.view().devices.is_removed(peer) {
                link::tell_removed(&self.inner, connection).await;
            } else {
                connection.close(1u32.into(), b"not a member");
            }
            return Ok(());
        }
        link::serve(self.inner.clone(), connection, peer).await;
        Ok(())
    }
}

/// Serves `beep-join/1`: desktops only.
#[derive(Clone)]
pub struct JoinProtocol {
    inner: Arc<Inner>,
}

impl Debug for JoinProtocol {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("JoinProtocol")
    }
}

impl ProtocolHandler for JoinProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        crate::join::serve(&self.inner, connection).await;
        Ok(())
    }
}

/// A connection `own` cuts, and whether its mesh link tells the peer first.
struct Doomed {
    conn: Connection,
    told: bool,
}

/// Keeps one dialer per other member and cuts the links of devices that
/// are no longer members.
async fn own(inner: Arc<Inner>) {
    let mut dialers: JoinSet<EndpointId> = JoinSet::new();
    let mut running: BTreeMap<EndpointId, AbortHandle> = BTreeMap::new();
    let mut closers: JoinSet<usize> = JoinSet::new();
    let mut cutting: BTreeSet<usize> = BTreeSet::new();
    let mut rev = inner.rev.subscribe();
    loop {
        rev.borrow_and_update();
        let view = inner.view();
        let member = view.devices.is_member(inner.me);
        let wanted: Vec<EndpointId> = if member {
            view.devices
                .members()
                .into_iter()
                .map(|d| d.id)
                .filter(|id| *id != inner.me)
                .collect()
        } else {
            for handle in running.values() {
                handle.abort();
            }
            Vec::new()
        };
        // A dialer for a removed member ends on its own once its link has
        // told the peer; aborting it would cut that short.
        running.retain(|id, _| wanted.contains(id));
        for id in wanted {
            running
                .entry(id)
                .or_insert_with(|| dialers.spawn(dial(inner.clone(), id)));
        }
        let doomed: Vec<Doomed> = inner
            .conns()
            .iter()
            .filter(|(id, _)| !member || !view.devices.is_member(**id))
            .map(|(_, live)| Doomed {
                conn: live.conn.clone(),
                told: true,
            })
            .chain(
                inner
                    .retired()
                    .values()
                    .filter(|c| !member || !view.devices.is_member(c.remote_id()))
                    .map(|c| Doomed {
                        conn: c.clone(),
                        told: false,
                    }),
            )
            .collect();
        for Doomed { conn, told } in doomed {
            let id = conn.stable_id();
            if cutting.insert(id) {
                // The link tells the peer first when it can; this ends it
                // when the peer will not read, and with it the peer's streams.
                let wait = if member && told {
                    GRACE
                } else {
                    Duration::ZERO
                };
                closers.spawn(async move {
                    sleep(wait).await;
                    conn.close(2u32.into(), b"removed");
                    id
                });
            }
        }
        tokio::select! {
            changed = rev.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            Some(ended) = dialers.join_next() => match ended {
                Ok(id) => { running.remove(&id); }
                Err(e) if e.is_cancelled() => {}
                Err(e) => warn!("a dialer failed: {e}"),
            },
            Some(ended) = closers.join_next() => {
                if let Ok(id) = ended {
                    cutting.remove(&id);
                }
            }
        }
    }
}

/// Keeps a connection to `peer` up while it is a member and so are we.
async fn dial(inner: Arc<Inner>, peer: EndpointId) -> EndpointId {
    let mut backoff = Backoff(FIRST_RETRY);
    let mut waited = false;
    loop {
        let notified = inner.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !inner.is_member(peer) || !inner.is_member(inner.me) || inner.is_closed() {
            return peer;
        }
        if let Some(conn) = inner.live(peer) {
            tokio::select! {
                () = &mut notified => {}
                _ = conn.closed() => {}
            }
            continue;
        }
        if inner.me > peer && !waited {
            waited = true;
            tokio::select! {
                () = &mut notified => {}
                () = sleep(HEAD_START) => {}
            }
            continue;
        }
        let tried = inner.addrs().get(&peer).cloned();
        let addr = tried
            .as_ref()
            .map_or_else(|| EndpointAddr::new(peer), |n| n.addr.clone());
        let started = Instant::now();
        let connect = timeout(DIAL_TIMEOUT, inner.endpoint.connect(addr, PEER_ALPN));
        tokio::pin!(connect);
        // Unrelated changes wake us; only a link that appeared cancels the dial.
        let dialed = loop {
            tokio::select! {
                dialed = &mut connect => break Some(dialed),
                () = &mut notified => {
                    if inner.live(peer).is_some() {
                        break None;
                    }
                    notified.set(inner.wake.notified());
                    notified.as_mut().enable();
                }
            }
        };
        let Some(dialed) = dialed else { continue };
        match dialed {
            Ok(Ok(conn)) => {
                link::serve(inner.clone(), conn, inner.me).await;
                waited = false;
                if started.elapsed() >= STABLE {
                    backoff.reset();
                    continue;
                }
            }
            Ok(Err(e)) => debug!("dial failed: {e}"),
            Err(_) => debug!("dial timed out"),
        }
        // Only a new address for this peer ends the wait early.
        let pause = sleep(backoff.pause());
        tokio::pin!(pause);
        loop {
            let moved = inner.addr_wake.notified();
            tokio::pin!(moved);
            moved.as_mut().enable();
            let now = inner.addrs().get(&peer).cloned();
            if now != tried {
                break;
            }
            tokio::select! {
                () = &mut pause => break,
                () = &mut moved => {}
            }
        }
    }
}
