//! `Mesh`: one device's membership. An owner task keeps a dialer per other
//! member; `link` runs each connection; `join` handles tickets.

use crate::store::{Core, SharedView, Store, View};
use crate::{Device, DeviceKind, JoinTicket, Key, MeshError, link};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore, broadcast, watch};
use tokio::task::{AbortHandle, JoinHandle, JoinSet};
use tokio::time::{Instant, sleep, timeout};
use tracing::{debug, warn};

pub const PEER_ALPN: &[u8] = b"beep-peer/1";
pub const JOIN_ALPN: &[u8] = b"beep-join/1";

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(12);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
/// A connection that lasted this long counts as having worked.
const STABLE: Duration = Duration::from_secs(5);
const CHANGES: usize = 256;
/// Further streams one peer may have open at once.
pub(crate) const MAX_STREAMS: usize = 64;
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
    pub send: iroh::endpoint::SendStream,
    pub recv: iroh::endpoint::RecvStream,
}

pub type StreamHandler =
    Arc<dyn Fn(PeerStream) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

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
}

pub(crate) struct Inner {
    pub endpoint: Endpoint,
    pub me: EndpointId,
    pub store: Store,
    view: SharedView,
    pub rev: Arc<watch::Sender<u64>>,
    pub changes: broadcast::Sender<Key>,
    pub conns: Mutex<BTreeMap<EndpointId, Live>>,
    pub addrs: Mutex<BTreeMap<EndpointId, EndpointAddr>>,
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

    pub fn conns(&self) -> std::sync::MutexGuard<'_, BTreeMap<EndpointId, Live>> {
        self.conns.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn addrs(&self) -> std::sync::MutexGuard<'_, BTreeMap<EndpointId, EndpointAddr>> {
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

    /// Addresses to hand a peer: ours and the ones we have seen.
    pub fn addr_list(&self) -> Vec<EndpointAddr> {
        let mut list: Vec<_> = self.addrs().values().cloned().collect();
        list.push(self.endpoint.addr());
        list.truncate(MAX_ADDRS);
        list
    }

    /// Remembers where members were last reached.
    pub async fn learn(&self, list: Vec<EndpointAddr>) {
        let view = self.view();
        let mut changed = false;
        {
            let mut known = self.addrs();
            for addr in list.into_iter().take(MAX_ADDRS) {
                if addr.id == self.me || !view.devices.is_member(addr.id) {
                    continue;
                }
                if known.get(&addr.id) != Some(&addr) {
                    known.insert(addr.id, addr);
                    changed = true;
                }
            }
        }
        if changed {
            let all: Vec<_> = self.addrs().values().cloned().collect();
            if let Err(e) = self.store.run(move |core| core.save_addrs(&all)).await {
                debug!("could not save peer addresses: {e}");
            }
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
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Relaxed);
        self.owner.abort();
        for live in self.inner.conns().values() {
            live.conn.close(0u32.into(), b"closed");
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
        addrs.push(ticket.addr.clone());
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

    fn run(endpoint: Endpoint, core: Core, addrs: Vec<EndpointAddr>) -> Result<Mesh, MeshError> {
        let me = endpoint.id();
        let view = core.view();
        let rev = core.rev();
        let known = addrs
            .into_iter()
            .filter(|a| a.id != me)
            .map(|a| (a.id, a))
            .collect();
        let inner = Arc::new(Inner {
            endpoint,
            me,
            store: Store::spawn(core)?,
            view,
            rev,
            changes: broadcast::channel(CHANGES).0,
            conns: Mutex::default(),
            addrs: Mutex::new(known),
            wake: Notify::new(),
            handler: Mutex::new(None),
            joins: Semaphore::new(8),
            closed: AtomicBool::new(false),
        });
        let owner: JoinHandle<()> = tokio::spawn(own(inner.clone()));
        Ok(Mesh {
            _guard: Arc::new(Guard {
                inner: inner.clone(),
                owner: owner.abort_handle(),
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
                        addrs.get(&device.id).cloned()
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

    /// Keys that changed, written here or taken from a peer.
    pub fn changes(&self) -> broadcast::Receiver<Key> {
        self.inner.changes.subscribe()
    }

    /// Rises whenever the device list, the state or who is online changes.
    pub fn updates(&self) -> watch::Receiver<u64> {
        self.inner.rev.subscribe()
    }

    pub fn peer(&self, id: EndpointId) -> Option<Connection> {
        self.inner.live(id)
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
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| MeshError::Closed)?
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

/// Keeps one dialer per other member.
async fn own(inner: Arc<Inner>) {
    let mut dialers: JoinSet<EndpointId> = JoinSet::new();
    let mut running: BTreeMap<EndpointId, AbortHandle> = BTreeMap::new();
    let mut rev = inner.rev.subscribe();
    loop {
        rev.borrow_and_update();
        let view = inner.view();
        let wanted: Vec<EndpointId> = if view.devices.is_member(inner.me) {
            view.devices
                .members()
                .into_iter()
                .map(|d| d.id)
                .filter(|id| *id != inner.me)
                .collect()
        } else {
            Vec::new()
        };
        // A dialer for a removed member ends on its own once its connection
        // has told the peer; aborting it would cut that short.
        running.retain(|id, _| wanted.contains(id));
        for id in wanted {
            running
                .entry(id)
                .or_insert_with(|| dialers.spawn(dial(inner.clone(), id)));
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
        }
    }
}

/// Keeps a connection to `peer` up while it is a member.
async fn dial(inner: Arc<Inner>, peer: EndpointId) -> EndpointId {
    let mut backoff = Backoff(FIRST_RETRY);
    loop {
        let notified = inner.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !inner.is_member(peer) || inner.is_closed() {
            return peer;
        }
        if let Some(conn) = inner.live(peer) {
            tokio::select! {
                () = &mut notified => {}
                _ = conn.closed() => {}
            }
            continue;
        }
        let addr = inner
            .addrs()
            .get(&peer)
            .cloned()
            .unwrap_or_else(|| EndpointAddr::new(peer));
        let started = Instant::now();
        let dialed = tokio::select! {
            () = &mut notified => continue,
            dialed = timeout(DIAL_TIMEOUT, inner.endpoint.connect(addr, PEER_ALPN)) => dialed,
        };
        match dialed {
            Ok(Ok(conn)) => {
                link::serve(inner.clone(), conn, inner.me).await;
                if started.elapsed() >= STABLE {
                    backoff.reset();
                    continue;
                }
            }
            Ok(Err(e)) => debug!("dial failed: {e}"),
            Err(_) => debug!("dial timed out"),
        }
        let notified = inner.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        tokio::select! {
            () = &mut notified => {}
            () = sleep(backoff.pause()) => {}
        }
    }
}
