//! One connection to a peer. The first bi stream (opened by whoever
//! dialed) is the sync stream: both sides send `Hello`, then their device
//! ops, then the entries the other lacks, then pushes on every change.
//! Further bi streams go to the handler set with `on_stream`.
//!
//! Device ops are merged before entries are applied, because `apply`
//! checks origins against the list. Sync never decides "in sync" from
//! heads: it sends `since(what the peer holds)` and keeps going.

use crate::frame::{self, MAX_FRAME};
use crate::mesh::{Inner, Live, MAX_STREAMS, PeerStream};
use crate::{DeviceOp, Entry, Heads, MeshError};
use iroh::EndpointAddr;
use iroh::EndpointId;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{debug, warn};

const OPS_PER_FRAME: usize = 128;
const ENTRIES_PER_FRAME: usize = 256;
const BYTES_PER_FRAME: usize = 512 * 1024;
const SETUP: Duration = Duration::from_secs(10);
const FLUSH: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize)]
enum Frame {
    Hello {
        addrs: Vec<EndpointAddr>,
        heads: Heads,
    },
    Ops(Vec<DeviceOp>),
    /// Addresses that are new since the last send.
    Addrs(Vec<EndpointAddr>),
    Entries(Vec<Entry>),
    /// The sender removed the receiver.
    Removed,
}

/// What the peer is known to hold, per origin.
type Known = Arc<Mutex<Heads>>;

/// Runs a connection until it ends, then unregisters it.
pub(crate) async fn serve(inner: Arc<Inner>, conn: Connection, dialer: EndpointId) {
    let peer = conn.remote_id();
    if !register(&inner, &conn, peer, dialer) {
        conn.close(3u32.into(), b"duplicate");
        return;
    }
    if let Err(e) = run(&inner, &conn, peer, dialer).await {
        debug!("link ended: {e}");
    }
    conn.close(0u32.into(), b"done");
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
        let (mut send, _recv) = conn.accept_bi().await.map_err(frame::net)?;
        let ops = inner.store.run(|core| Ok(core.ops())).await?;
        send_ops(&mut send, &ops).await?;
        frame::send(&mut send, &Frame::Removed).await?;
        send.finish().map_err(frame::net)?;
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

/// Keeps one connection per peer: the one the smaller id dialed.
fn register(inner: &Inner, conn: &Connection, peer: EndpointId, dialer: EndpointId) -> bool {
    let mut conns = inner.conns();
    if let Some(old) = conns.get(&peer)
        && old.conn.close_reason().is_none()
    {
        if old.dialer <= dialer {
            return false;
        }
        old.conn.close(3u32.into(), b"duplicate");
    }
    conns.insert(
        peer,
        Live {
            conn: conn.clone(),
            dialer,
        },
    );
    drop(conns);
    inner.bump();
    true
}

async fn run(
    inner: &Arc<Inner>,
    conn: &Connection,
    peer: EndpointId,
    dialer: EndpointId,
) -> Result<(), MeshError> {
    let opened = async {
        if dialer == inner.me {
            conn.open_bi().await.map_err(frame::net)
        } else {
            conn.accept_bi().await.map_err(frame::net)
        }
    };
    let (send, recv) = timeout(SETUP, opened)
        .await
        .map_err(|_| frame::net("no sync stream"))??;
    let known: Known = Arc::default();
    let (hello_tx, hello_rx) = oneshot::channel();
    let mut tasks: JoinSet<Result<(), MeshError>> = JoinSet::new();
    tasks.spawn(read(
        inner.clone(),
        conn.clone(),
        recv,
        known.clone(),
        hello_tx,
    ));
    tasks.spawn(write(
        inner.clone(),
        conn.clone(),
        peer,
        send,
        known,
        hello_rx,
    ));
    tasks.spawn(streams(inner.clone(), conn.clone(), peer));
    // The first task to end ends the link; dropping the set stops the rest.
    match tasks.join_next().await {
        Some(Ok(result)) => result,
        Some(Err(e)) => Err(frame::net(e)),
        None => Ok(()),
    }
}

async fn read(
    inner: Arc<Inner>,
    conn: Connection,
    mut recv: RecvStream,
    known: Known,
    hello_tx: oneshot::Sender<()>,
) -> Result<(), MeshError> {
    let mut hello_tx = Some(hello_tx);
    while let Some(frame) = frame::recv::<Frame>(&mut recv, MAX_FRAME).await? {
        match frame {
            Frame::Hello { addrs, heads } => {
                inner.learn(addrs).await;
                *known.lock().unwrap_or_else(PoisonError::into_inner) = heads;
                if let Some(tx) = hello_tx.take() {
                    // The writer is gone only when the link is ending.
                    tx.send(()).ok();
                }
            }
            Frame::Addrs(addrs) => inner.learn(addrs).await,
            Frame::Ops(ops) => {
                let merged = inner.store.run(move |core| core.merge(&ops)).await?;
                if !merged.rejected.is_empty() {
                    debug!("refused {} device ops from a peer", merged.rejected.len());
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
    }
    Ok(())
}

async fn send_ops(send: &mut SendStream, ops: &[DeviceOp]) -> Result<(), MeshError> {
    for chunk in ops.chunks(OPS_PER_FRAME) {
        frame::send(send, &Frame::Ops(chunk.to_vec())).await?;
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
        bytes += postcard::to_stdvec(&entry)?.len();
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
    let page = std::mem::take(page);
    {
        let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
        for e in &page {
            let head = known.entry(e.origin).or_insert(0);
            *head = (*head).max(e.seq);
        }
    }
    frame::send(send, &Frame::Entries(page)).await
}

async fn write(
    inner: Arc<Inner>,
    conn: Connection,
    peer: EndpointId,
    mut send: SendStream,
    known: Known,
    hello_rx: oneshot::Receiver<()>,
) -> Result<(), MeshError> {
    let mut rev = inner.rev.subscribe();
    let heads = inner.store.run(|core| Ok(core.heads())).await?;
    let first = inner.addr_list();
    let mut sent_addrs: BTreeMap<EndpointId, EndpointAddr> =
        first.iter().map(|a| (a.id, a.clone())).collect();
    let hello = Frame::Hello {
        addrs: first,
        heads,
    };
    frame::send(&mut send, &hello).await?;
    let ops = inner.store.run(|core| Ok(core.ops())).await?;
    let mut sent_ops = ops.len();
    send_ops(&mut send, &ops).await?;
    timeout(SETUP, hello_rx)
        .await
        .map_err(|_| frame::net("no hello"))?
        .map_err(|_| frame::net("link ended"))?;
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
            frame::send(&mut send, &Frame::Removed).await?;
            send.finish().map_err(frame::net)?;
            // The peer closes once it has read the news; a close sooner
            // would discard what it has not read.
            timeout(FLUSH, conn.closed()).await.ok();
            conn.close(2u32.into(), b"removed");
            return Ok(());
        }
        let fresh: Vec<_> = inner
            .addr_list()
            .into_iter()
            .filter(|a| sent_addrs.get(&a.id) != Some(a))
            .collect();
        if !fresh.is_empty() {
            sent_addrs.extend(fresh.iter().map(|a| (a.id, a.clone())));
            frame::send(&mut send, &Frame::Addrs(fresh)).await?;
        }
        let theirs = known.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let entries = inner.store.run(move |core| Ok(core.since(&theirs))).await?;
        send_entries(&mut send, entries, &known).await?;
        if rev.changed().await.is_err() {
            return Ok(());
        }
    }
}

async fn streams(inner: Arc<Inner>, conn: Connection, peer: EndpointId) -> Result<(), MeshError> {
    let permits = Arc::new(Semaphore::new(MAX_STREAMS));
    let mut running = JoinSet::new();
    loop {
        tokio::select! {
            accepted = conn.accept_bi() => {
                let (mut send, mut recv) = accepted.map_err(frame::net)?;
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
            }
        }
    }
}
