//! One connection to a peer. The first bi stream (opened by whoever
//! dialed) is the sync stream: both sides send `Hello`, then their device
//! ops, then the entries the other lacks, then pushes on every change.
//! Further bi streams go to the handler set with `on_stream`.
//!
//! Device ops are merged before entries are applied, because `apply`
//! checks origins against the list. Sync never decides "in sync" from
//! heads: it sends `since(what the peer holds)` and keeps going.

use crate::mesh::{AddrNote, Inner, Live, MAX_STREAMS, PeerStream};
use crate::wire::{self, FrameReader, MAX_FRAME};
use crate::{DeviceOp, Entry, Heads, MeshError};
use iroh::EndpointId;
use iroh::endpoint::{Connection, SendStream};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::mem::take;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until, timeout};
use tracing::{debug, warn};

const OPS_PER_FRAME: usize = 128;
const ENTRIES_PER_FRAME: usize = 256;
const BYTES_PER_FRAME: usize = 512 * 1024;
const SETUP: Duration = Duration::from_secs(10);
const FLUSH: Duration = Duration::from_secs(2);
/// Refused items on one link before it is dropped.
const MAX_REFUSED: usize = 16;
/// How long a connection that left the mesh link keeps taking streams the
/// peer may already have opened on it.
const DRAIN: Duration = Duration::from_secs(3);
/// How long it stays after its last stream ends, so the peer can read the
/// end of it before the connection goes.
const LINGER: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize)]
pub(crate) enum Frame {
    /// Both sides send one first, before anything else.
    Hello {
        addrs: Vec<AddrNote>,
        #[serde(with = "crate::wire::heads")]
        heads: Heads,
        /// Random per start of a `Mesh`: a new one means the peer restarted.
        session: u64,
    },
    Ops(Vec<DeviceOp>),
    /// Addresses that are new since the last send.
    Addrs(Vec<AddrNote>),
    Entries(Vec<Entry>),
    /// The sender removed the receiver.
    Removed,
}

/// What the peer is known to hold, per origin.
type Known = Arc<Mutex<Heads>>;

/// Runs a connection until it ends, then unregisters it. The connection is
/// not closed here: the streams on it end first (see `run`).
pub(crate) async fn serve(inner: Arc<Inner>, conn: Connection, dialer: EndpointId) {
    let peer = conn.remote_id();
    if let Err(e) = run(&inner, &conn, peer, dialer).await {
        debug!("link ended: {e}");
    }
    unregister(&inner, &conn, peer);
}

fn unregister(inner: &Inner, conn: &Connection, peer: EndpointId) {
    let mut conns = inner.conns();
    if conns
        .get(&peer)
        .is_some_and(|l| l.conn.stable_id() == conn.stable_id())
    {
        conns.remove(&peer);
    }
    drop(conns);
    inner.bump();
}

/// A removed device that dials in is told so: it gets the ops (which hold
/// its `Remove`) and a `Removed`, then the connection closes.
pub(crate) async fn tell_removed(inner: &Inner, conn: Connection) {
    let work = async {
        let (mut send, _recv) = conn.accept_bi().await.map_err(wire::net)?;
        let hello = Frame::Hello {
            addrs: Vec::new(),
            heads: Heads::new(),
            session: inner.session,
        };
        wire::send(&mut send, &hello).await?;
        let ops = inner.store.run(|core| Ok(core.ops())).await?;
        send_ops(&mut send, &ops).await?;
        wire::send(&mut send, &Frame::Removed).await?;
        send.finish().map_err(wire::net)?;
        timeout(FLUSH, conn.closed()).await.ok();
        Ok::<(), MeshError>(())
    };
    match timeout(SETUP, work).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => debug!("could not tell a removed device: {e}"),
        Err(_) => debug!("telling a removed device timed out"),
    }
    conn.close(1u32.into(), b"removed");
}

/// Keeps one connection per peer. When both sides of one instance dialed,
/// the one the smaller id dialed stays; the other one is retired, not
/// closed, so streams already open on it carry on. A connection from a new
/// instance of the peer (a restart) always replaces the old, which may be a
/// dead link. Returns what retires the new connection, or `None` when it
/// lost the tie itself.
fn register(
    inner: &Inner,
    conn: &Connection,
    peer: EndpointId,
    who: Dialed,
    session: u64,
) -> Option<Arc<Notify>> {
    let mut conns = inner.conns();
    if let Some(old) = conns.get(&peer)
        && old.conn.close_reason().is_none()
    {
        if old.session == session && old.dialer < who.0 {
            return None;
        }
        if old.session == session {
            old.retire.notify_one();
        } else {
            old.conn.close(3u32.into(), b"duplicate");
        }
    }
    let retire = Arc::new(Notify::new());
    conns.insert(
        peer,
        Live {
            conn: conn.clone(),
            dialer: who.0,
            session,
            retire: retire.clone(),
        },
    );
    drop(conns);
    inner.bump();
    Some(retire)
}

/// Who opened a connection.
#[derive(Clone, Copy)]
struct Dialed(EndpointId);

async fn run(
    inner: &Arc<Inner>,
    conn: &Connection,
    peer: EndpointId,
    dialer: EndpointId,
) -> Result<(), MeshError> {
    let Shaken {
        send,
        recv,
        heads,
        session,
        first,
    } = match handshake(inner, conn, peer, dialer).await {
        Ok(done) => done,
        Err(e) => {
            conn.close(0u32.into(), b"done");
            return Err(e);
        }
    };
    // Only now can a caller reach the connection through `peer()`, so no
    // stream of theirs can be taken for the sync stream.
    let retire = register(inner, conn, peer, Dialed(dialer), session);
    let (over, over_rx) = watch::channel(false);
    let mut app = JoinSet::new();
    app.spawn(streams(inner.clone(), conn.clone(), peer, over_rx));
    let result = match retire {
        Some(retire) => {
            let known: Known = Arc::new(Mutex::new(heads));
            let mut tasks: JoinSet<Result<(), MeshError>> = JoinSet::new();
            tasks.spawn(read(inner.clone(), conn.clone(), peer, recv, known.clone()));
            tasks.spawn(write(inner.clone(), conn.clone(), peer, send, known, first));
            // The first to end ends the sync link; dropping the set stops the rest.
            tokio::select! {
                ended = tasks.join_next() => match ended {
                    Some(Ok(result)) => result,
                    Some(Err(e)) => Err(wire::net(e)),
                    None => Ok(()),
                },
                // The streams end with the connection.
                _ = app.join_next() => Ok(()),
                () = retire.notified() => Ok(()),
            }
        }
        None => Ok(()),
    };
    // The sync link is over. Streams on the connection carry on until they
    // end (see `streams`), so it is no longer reachable but not closed; a
    // removal still cuts it, through `retired`.
    inner.retired().insert(conn.stable_id(), conn.clone());
    unregister(inner, conn, peer);
    if !inner.is_member(inner.me) || !inner.is_member(peer) {
        conn.close(2u32.into(), b"removed");
    }
    over.send(true).ok();
    while app.join_next().await.is_some() {}
    inner.retired().remove(&conn.stable_id());
    result
}

/// The sync stream after both hellos.
struct Shaken {
    send: SendStream,
    recv: FrameReader,
    /// What the peer holds.
    heads: Heads,
    session: u64,
    /// The addresses we sent in our hello.
    first: Vec<AddrNote>,
}

async fn handshake(
    inner: &Arc<Inner>,
    conn: &Connection,
    peer: EndpointId,
    dialer: EndpointId,
) -> Result<Shaken, MeshError> {
    let opened = async {
        if dialer == inner.me {
            conn.open_bi().await.map_err(wire::net)
        } else {
            conn.accept_bi().await.map_err(wire::net)
        }
    };
    let (mut send, recv) = timeout(SETUP, opened)
        .await
        .map_err(|_| wire::net("no sync stream"))??;
    let mut recv = FrameReader::new(recv, MAX_FRAME);
    let heads = inner.store.run(|core| Ok(core.heads())).await?;
    let first = inner.addr_list();
    let hello = Frame::Hello {
        addrs: first.clone(),
        heads,
        session: inner.session,
    };
    wire::send(&mut send, &hello).await?;
    let theirs = timeout(SETUP, recv.recv::<Frame>())
        .await
        .map_err(|_| wire::net("no hello"))??;
    let Some(Frame::Hello {
        addrs,
        heads,
        session,
    }) = theirs
    else {
        return Err(wire::net("the first frame was not a hello"));
    };
    inner.learn(addrs, peer).await;
    Ok(Shaken {
        send,
        recv,
        heads,
        session,
        first,
    })
}

async fn read(
    inner: Arc<Inner>,
    conn: Connection,
    peer: EndpointId,
    mut recv: FrameReader,
    known: Known,
) -> Result<(), MeshError> {
    let mut refused = 0;
    while let Some(frame) = recv.recv::<Frame>().await? {
        match frame {
            Frame::Hello { .. } => return Err(wire::net("a second hello")),
            Frame::Addrs(addrs) => inner.learn(addrs, peer).await,
            Frame::Ops(ops) => {
                let merged = inner.store.run(move |core| core.merge(&ops)).await?;
                if !merged.rejected.is_empty() {
                    debug!("refused {} device ops from a peer", merged.rejected.len());
                    refused += merged.rejected.len();
                }
            }
            Frame::Entries(entries) => {
                {
                    let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
                    for e in &entries {
                        let head = known.entry(e.origin).or_insert(0);
                        *head = (*head).max(e.seq);
                    }
                }
                let applied = inner.store.run(move |core| core.apply(entries)).await?;
                if !applied.refused.is_empty() {
                    warn!(
                        "refused entries from a peer in {} batches",
                        applied.refused.len()
                    );
                    refused += applied.refused.len();
                }
                for key in applied.changed {
                    // No receiver is a normal state.
                    inner.changes.send(key).ok();
                }
            }
            Frame::Removed => {
                conn.close(2u32.into(), b"removed");
                return Ok(());
            }
        }
        if refused >= MAX_REFUSED {
            conn.close(1u32.into(), b"too many refused items");
            return Err(wire::net("too many refused items"));
        }
    }
    Ok(())
}

async fn send_ops(send: &mut SendStream, ops: &[DeviceOp]) -> Result<(), MeshError> {
    for chunk in ops.chunks(OPS_PER_FRAME) {
        wire::send(send, &Frame::Ops(chunk.to_vec())).await?;
    }
    Ok(())
}

/// Entries in frames of bounded size, then marks them held by the peer.
async fn send_entries(
    send: &mut SendStream,
    entries: Vec<Entry>,
    known: &Known,
) -> Result<(), MeshError> {
    let mut page = Vec::new();
    let mut bytes = 0;
    for entry in entries {
        bytes += rmp_serde::to_vec(&entry)?.len();
        page.push(entry);
        if page.len() >= ENTRIES_PER_FRAME || bytes >= BYTES_PER_FRAME {
            flush(send, &mut page, known).await?;
            bytes = 0;
        }
    }
    flush(send, &mut page, known).await
}

async fn flush(
    send: &mut SendStream,
    page: &mut Vec<Entry>,
    known: &Known,
) -> Result<(), MeshError> {
    if page.is_empty() {
        return Ok(());
    }
    let page = take(page);
    {
        let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
        for e in &page {
            let head = known.entry(e.origin).or_insert(0);
            *head = (*head).max(e.seq);
        }
    }
    wire::send(send, &Frame::Entries(page)).await
}

async fn write(
    inner: Arc<Inner>,
    conn: Connection,
    peer: EndpointId,
    mut send: SendStream,
    known: Known,
    first: Vec<AddrNote>,
) -> Result<(), MeshError> {
    let mut rev = inner.rev.subscribe();
    let mut sent_addrs: BTreeMap<EndpointId, AddrNote> =
        first.iter().map(|n| (n.addr.id, n.clone())).collect();
    let ops = inner.store.run(|core| Ok(core.ops())).await?;
    let mut sent_ops = ops.len();
    send_ops(&mut send, &ops).await?;
    loop {
        rev.borrow_and_update();
        let view = inner.view();
        let ops = view.devices.ops();
        if sent_ops < ops.len() {
            send_ops(&mut send, &ops[sent_ops..]).await?;
            sent_ops = ops.len();
        }
        if !view.devices.is_member(inner.me) {
            conn.close(2u32.into(), b"removed");
            return Ok(());
        }
        if !view.devices.is_member(peer) {
            // The peer closes once it has read the news; a close sooner
            // would discard what it has not read. A peer that stalls is
            // cut off by `own` after a grace period either way.
            let tell = async {
                wire::send(&mut send, &Frame::Removed).await?;
                send.finish().map_err(wire::net)?;
                conn.closed().await;
                Ok::<(), MeshError>(())
            };
            timeout(FLUSH, tell).await.ok();
            conn.close(2u32.into(), b"removed");
            return Ok(());
        }
        let fresh: Vec<_> = inner
            .addr_list()
            .into_iter()
            .filter(|n| sent_addrs.get(&n.addr.id) != Some(n))
            .collect();
        if !fresh.is_empty() {
            sent_addrs.extend(fresh.iter().map(|n| (n.addr.id, n.clone())));
            wire::send(&mut send, &Frame::Addrs(fresh)).await?;
        }
        let theirs = known.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let entries = inner.store.run(move |core| Ok(core.since(&theirs))).await?;
        send_entries(&mut send, entries, &known).await?;
        if rev.changed().await.is_err() {
            return Ok(());
        }
    }
}

/// Takes the streams a peer opens after the sync stream. Once `over` says
/// the sync link is done (replaced by another connection, or ended), it
/// keeps taking streams for `DRAIN`, then returns when none are left running
/// for `LINGER`. It returns early when the connection closes.
async fn streams(
    inner: Arc<Inner>,
    conn: Connection,
    peer: EndpointId,
    mut over: watch::Receiver<bool>,
) -> Result<(), MeshError> {
    let permits = Arc::new(Semaphore::new(MAX_STREAMS));
    let mut running = JoinSet::new();
    let mut until: Option<Instant> = None;
    loop {
        if until.is_none() && *over.borrow() {
            until = Some(Instant::now() + DRAIN);
        }
        if until.is_some_and(|t| running.is_empty() && Instant::now() >= t) {
            return Ok(());
        }
        tokio::select! {
            accepted = conn.accept_bi() => {
                let (mut send, mut recv) = accepted.map_err(wire::net)?;
                let handler = inner
                    .handler
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                let kind = inner.view().devices.members().into_iter().find(|d| d.id == peer).map(|d| d.kind);
                let (Some(handler), Some(kind), Ok(permit)) =
                    (handler, kind, permits.clone().try_acquire_owned())
                else {
                    // Refusing is the answer; the peer sees the reset.
                    send.reset(1u32.into()).ok();
                    recv.stop(1u32.into()).ok();
                    continue;
                };
                running.spawn(async move {
                    let _permit = permit;
                    handler(PeerStream { peer, kind, send, recv }).await;
                });
            }
            Some(ended) = running.join_next() => {
                if let Err(e) = ended
                    && !e.is_cancelled()
                {
                    warn!("a peer stream task failed: {e}");
                }
                if until.is_some() {
                    until = Some(Instant::now() + LINGER);
                }
            }
            changed = over.changed(), if until.is_none() => {
                if changed.is_err() {
                    until = Some(Instant::now() + DRAIN);
                }
            }
            () = sleep_until(until.unwrap_or_else(Instant::now)), if until.is_some() && running.is_empty() => {}
        }
    }
}
