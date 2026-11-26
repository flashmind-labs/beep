//! What the mesh shares with its links: the connections, addresses and
//! channel slots behind one `Mesh`. `link` and `join` work on it, and `mesh`
//! owns it and sits above them.

mod channels;

use crate::clock::now_ms;
pub(crate) use crate::store::AddrNote;
use crate::store::{Core, SharedView, Store, View};
use crate::{DeviceKind, Key, MeshError};
use channels::Channels;
pub(crate) use channels::{CachedConnection, FailedDial};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointId};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;
use tokio::sync::{Notify, Semaphore, broadcast, mpsc, watch};
use tracing::debug;

const CHANGES: usize = 256;
/// How far ahead of our clock a relayed address counter may be.
const MAX_AHEAD_MS: u64 = 60_000;
/// Further streams one peer may have open at once.
pub(crate) const MAX_STREAMS: usize = 64;
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
    /// Counts against the peer's open streams until this is dropped.
    pub(crate) _lease: Lease,
}

/// One open stream of a link, counted until dropped.
pub(crate) struct Lease(Arc<watch::Sender<usize>>);

impl Lease {
    pub(crate) fn take(open: &Arc<watch::Sender<usize>>) -> Self {
        open.send_modify(|n| *n += 1);
        Self(open.clone())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.send_modify(|n| *n -= 1);
    }
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
    pub channels: Channels,
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
    /// Where the streams peers open after the sync stream go: to the last
    /// reader of `Mesh::incoming`. None until there is one.
    incoming: Mutex<Option<mpsc::Sender<PeerStream>>>,
    pub joins: Semaphore,
    pub closed: AtomicBool,
}

impl Inner {
    pub(crate) fn open(
        endpoint: Endpoint,
        core: Core,
        addrs: Vec<AddrNote>,
    ) -> Result<Arc<Inner>, MeshError> {
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
        Ok(Arc::new(Inner {
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
            incoming: Mutex::new(None),
            joins: Semaphore::new(8),
            closed: AtomicBool::new(false),
        }))
    }

    /// A new queue for the streams peers open; the one before it, if any,
    /// ends.
    pub(crate) fn take_incoming(&self) -> mpsc::Receiver<PeerStream> {
        let (sender, receiver) = mpsc::channel(MAX_STREAMS);
        *self.incoming.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
        receiver
    }

    /// Hands `stream` to the reader of `Mesh::incoming`; gives it back when
    /// there is none or its queue is full.
    pub(crate) fn offer_stream(&self, stream: PeerStream) -> Result<(), PeerStream> {
        let sender = self
            .incoming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();

        match sender {
            Some(sender) => sender
                .try_send(stream)
                .map_err(mpsc::error::TrySendError::into_inner),
            None => Err(stream),
        }
    }

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

impl Inner {
    pub(crate) fn channel_connections(&self) -> Vec<Connection> {
        self.channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .flat_map(|slot| {
                let outgoing = slot.live(None);
                let incoming = slot.incoming.lock().unwrap_or_else(PoisonError::into_inner);
                outgoing
                    .into_iter()
                    .chain(incoming.iter().map(|cached| cached.connection.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(crate) fn restart_connections(&self, peer: EndpointId, session: u64) {
        for retired in self.retired().values() {
            if retired.conn.remote_id() == peer && retired.session != session {
                retired.conn.close(3u32.into(), b"peer restarted");
            }
        }

        for ((id, _), slot) in self
            .channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            if *id == peer {
                slot.live(Some(session));
            }
        }
    }

    /// Closes every command and subscription connection and forgets failed
    /// dials, so the next call dials again at once.
    pub(crate) fn reset_channels(&self) {
        for slot in self
            .channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
        {
            if let Some(cached) = slot
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                cached.connection.close(0u32.into(), b"slept");
            }

            for cached in slot
                .incoming
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..)
            {
                cached.connection.close(0u32.into(), b"slept");
            }

            // A dial in progress holds the lock and records its own outcome.
            if let Ok(mut failed) = slot.dialing.try_lock() {
                *failed = None;
            }
        }
    }

    pub(crate) fn prune_channels(&self) {
        let conns = self.conns();
        self.channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(peer, _), slot| {
                // A restarted peer's new mesh hello invalidates its old channels too.
                slot.live(conns.get(peer).map(|live| live.session));
                let keep = self.is_member(self.me()) && self.is_member(*peer);
                if !keep {
                    if let Some(connection) = slot.live(None) {
                        connection.close(2u32.into(), b"removed");
                    }
                    for cached in slot
                        .incoming
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .drain(..)
                    {
                        cached.connection.close(2u32.into(), b"removed");
                    }
                }
                keep
            });
    }
}
