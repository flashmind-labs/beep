//! Node-to-node sync on `ALPN`: `pull` reads a peer's project lists directly
//! over iroh, so sync goes on while the server is down.

use super::{
    gitdir::{Checkout, Repo},
    project::{Cursor, Shared},
};
use crate::{AsyncMutex, lock, membership::Membership};
use anyhow::{Context, Result, bail};
use beep_mesh::{OPEN_TIMEOUT, RETRY};
use beep_proto::node::LinkRx;
use beep_proto::watched;
use iroh::{Endpoint, EndpointAddr, EndpointId, endpoint::Connection};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    task::spawn_blocking,
    time::{Instant, sleep, timeout},
};
use wire::{ObjectScope, ObjectWant, Reply, receive_objects};

mod branches;
pub(super) mod serve;
mod wire;

/// Every batch carries its scanned checkout. Earlier versions cannot
/// enforce that base when Git changes during a pull.
pub(crate) const ALPN: &[u8] = b"beep-sync/7";

/// The most active pulls placed onto a single pooled connection. Stays under
/// released peers' `MAX_STREAMS = 64` and Quinn's 100-bidi-stream limit so
/// auxiliary fetch streams (`GetGit`/`GetObjects`/`Get`) always have headroom.
const POOL_MAX_PULLS: usize = 48;
/// Longest backoff between failed pulls while a peer is online or unmeshed.
const MAX_PULL_RETRY: Duration = Duration::from_secs(60);
/// A pull that lasted at least this long resets its retry backoff.
const STABLE_PULL: Duration = Duration::from_secs(5);

struct PooledConn {
    conn: Connection,
    leases: usize,
    broken: bool,
}

type DialLock = Arc<AsyncMutex<()>>;

/// Shares `beep-sync/7` connections per peer across synced projects, bounded to
/// `POOL_MAX_PULLS` active pulls per connection and closed as soon as the last
/// pull lease drops.
#[derive(Clone, Default)]
pub(super) struct SyncPool {
    peers: Arc<Mutex<HashMap<EndpointId, Vec<PooledConn>>>>,
    dials: Arc<Mutex<HashMap<EndpointId, DialLock>>>,
}

/// A peer's dial lock in use. It leaves the map when the dial ends or is
/// cancelled, unless another dial waits on it.
struct DialEntry<'a> {
    dials: &'a Mutex<HashMap<EndpointId, DialLock>>,
    peer: EndpointId,
    dial: DialLock,
}

impl Drop for DialEntry<'_> {
    fn drop(&mut self) {
        let mut dials = lock(self.dials);

        // The map and this entry hold the lock: nobody else waits on it.
        if Arc::strong_count(&self.dial) == 2 {
            dials.remove(&self.peer);
        }
    }
}

struct SyncLease {
    pool: SyncPool,
    peer: EndpointId,
    conn: Connection,
}

impl SyncLease {
    fn connection(&self) -> &Connection {
        &self.conn
    }

    fn mark_broken(&self) {
        self.pool.leave(self.peer, &self.conn, true);
    }
}

impl Drop for SyncLease {
    fn drop(&mut self) {
        self.pool.leave(self.peer, &self.conn, false);
    }
}

impl SyncPool {
    fn try_reuse(&self, peer: EndpointId) -> Option<SyncLease> {
        let mut peers = lock(&self.peers);
        let list = peers.get_mut(&peer)?;
        list.retain(|held| held.leases > 0 || held.conn.close_reason().is_none());

        let found = list.iter_mut().find(|held| {
            !held.broken && held.leases < POOL_MAX_PULLS && held.conn.close_reason().is_none()
        });

        let lease = found.map(|held| {
            held.leases += 1;
            SyncLease {
                pool: self.clone(),
                peer,
                conn: held.conn.clone(),
            }
        });

        if list.is_empty() {
            peers.remove(&peer);
        }

        lease
    }

    async fn acquire(&self, endpoint: &Endpoint, peer: &EndpointAddr) -> Result<SyncLease> {
        if let Some(lease) = self.try_reuse(peer.id) {
            return Ok(lease);
        }

        let dial = lock(&self.dials).entry(peer.id).or_default().clone();
        let entry = DialEntry {
            dials: &self.dials,
            peer: peer.id,
            dial,
        };

        self.dial(&entry.dial, endpoint, peer).await
    }

    async fn dial(
        &self,
        dial: &DialLock,
        endpoint: &Endpoint,
        peer: &EndpointAddr,
    ) -> Result<SyncLease> {
        let _guard = dial.lock().await;

        if let Some(lease) = self.try_reuse(peer.id) {
            return Ok(lease);
        }

        let conn = timeout(OPEN_TIMEOUT, endpoint.connect(peer.clone(), ALPN))
            .await
            .context("could not reach the peer in time")??;
        lock(&self.peers)
            .entry(peer.id)
            .or_default()
            .push(PooledConn {
                conn: conn.clone(),
                leases: 1,
                broken: false,
            });

        Ok(SyncLease {
            pool: self.clone(),
            peer: peer.id,
            conn,
        })
    }

    /// A lease on `connection` ends, or with `broken` the connection is
    /// flagged and no pull is placed on it again. It closes once no lease
    /// is left.
    fn leave(&self, peer: EndpointId, connection: &Connection, broken: bool) {
        let close = {
            let mut peers = lock(&self.peers);
            let mut close = None;

            if let Some(list) = peers.get_mut(&peer) {
                if let Some(index) = list
                    .iter()
                    .position(|held| held.conn.stable_id() == connection.stable_id())
                {
                    if broken {
                        list[index].broken = true;
                    } else {
                        list[index].leases = list[index].leases.saturating_sub(1);
                    }

                    if list[index].leases == 0 {
                        close = Some(list.remove(index).conn);
                    }
                }

                if list.is_empty() {
                    peers.remove(&peer);
                }
            }

            close
        };

        if let Some(conn) = close {
            conn.close(0u32.into(), if broken { b"broken" } else { b"idle" });
        }
    }
}

/// Waits until `peer` has an established mesh connection when this node is in
/// a mesh. Returns `false` if the project stopped while waiting.
async fn wait_for_mesh_peer(shared: &Shared, mesh: &Membership, peer: EndpointId) -> bool {
    let mut membership = mesh.subscribe();

    loop {
        let active = watched::read_and_update(&mut membership, Clone::clone);
        let Some(active) = active else {
            return true;
        };

        if active.peer(peer).is_some() {
            return true;
        }

        // Peer is offline in the mesh: settle sync state instead of staying
        // in Syncing, and wait reactively for the mesh connection.
        shared.heard(&peer);
        let mut updates = active.updates();

        loop {
            if active.peer(peer).is_some() {
                return true;
            }

            tokio::select! {
                () = shared.stopped() => return false,
                changed = updates.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                changed = membership.changed() => {
                    if changed.is_err() {
                        return false;
                    }

                    break;
                }
            }
        }
    }
}

/// Pulls `shared`'s list from `peer` until the project stops. When in a mesh,
/// waits reactively for the mesh link to `peer` before opening a stream on the
/// shared `beep-sync/7` connection.
pub(super) async fn pull(
    shared: Arc<Shared>,
    endpoint: Endpoint,
    pool: SyncPool,
    mesh: Membership,
    peer: EndpointAddr,
) {
    let mut again = shared.watch_pull_again();
    let mut backoff = RETRY;

    loop {
        if !wait_for_mesh_peer(&shared, &mesh, peer.id).await {
            return;
        }

        again.mark_unchanged();
        let started = Instant::now();
        let pulled = tokio::select! {
            pulled = pull_once(&shared, &endpoint, &pool, &peer) => pulled,
            () = shared.stopped() => return,
        };

        let reset = match &pulled {
            Ok(()) => started.elapsed() >= STABLE_PULL,
            Err(e) => {
                log::debug!("pulling {} from {}: {e:#}", shared.name(), peer.id);
                false
            }
        };

        shared.heard(&peer.id);

        if reset {
            backoff = RETRY;
        }

        // If the peer dropped offline in the mesh, go straight back to waiting
        // reactively instead of burning retry timers.
        if mesh
            .get()
            .is_some_and(|active| active.peer(peer.id).is_none())
        {
            backoff = RETRY;
            continue;
        }

        let wait = backoff;
        tokio::select! {
            () = sleep(wait) => {
                backoff = (backoff * 2).min(MAX_PULL_RETRY);
            }
            _ = again.changed() => {
                backoff = RETRY;
            }
            () = shared.stopped() => return,
        }
    }
}

async fn pull_once(
    shared: &Shared,
    endpoint: &Endpoint,
    pool: &SyncPool,
    peer: &EndpointAddr,
) -> Result<()> {
    let started = Instant::now();
    let lease = pool.acquire(endpoint, peer).await?;
    let connection = lease.connection();
    let opened = branches::open_pull(connection, shared, &peer.id).await;
    let (_tx, mut rx, mut first) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            lease.mark_broken();
            return Err(error);
        }
    };
    let result = pull_loop(shared, connection, peer, started, &mut rx, &mut first).await;

    if result.is_err() && connection.close_reason().is_some() {
        lease.mark_broken();
    }

    result
}

async fn pull_loop(
    shared: &Shared,
    connection: &Connection,
    peer: &EndpointAddr,
    started: Instant,
    rx: &mut LinkRx,
    first: &mut Option<Reply>,
) -> Result<()> {
    let mut last = None;
    let mut heard_answer = false;
    let mut again = shared.watch_pull_again();
    again.mark_unchanged();

    loop {
        let reply = if let Some(first) = first.take() {
            Some(first)
        } else {
            tokio::select! {
                reply = rx.recv::<Reply>() => reply?,
                // Asked to start over: dropping the stream ends this pull only.
                _ = again.changed() => return Ok(()),
            }
        };
        let Some(reply) = reply else {
            if !heard_answer {
                bail!("peer closed sync pull stream before answering");
            }

            break;
        };
        heard_answer = true;

        match reply {
            Reply::Branches { tips } => {
                branches::receive_catalog(connection, shared, &peer.id, tips).await?;
            }
            Reply::Head {
                commit,
                behind,
                has_git,
                branch,
            } => {
                log::debug!(
                    "{}: {} answered {:?} after dialling",
                    shared.name(),
                    peer.id,
                    started.elapsed()
                );
                let checkout = Checkout {
                    commit,
                    present: has_git,
                };
                shared.observe_head(&peer.id, checkout, branch.clone());
                let own = shared.head().await?;
                if own.differs(checkout)
                    && !shared.copies_git()
                    && !shared.branch_aware(&peer.id)
                    && let (Some(target), Some(remote_branch)) = (commit, branch.as_deref())
                {
                    let root = shared.root().to_owned();
                    let remote_branch = remote_branch.to_owned();
                    let peer_id = peer.id;
                    let (need, haves) = spawn_blocking(move || -> Result<_> {
                        let repo = Repo::open(&root).context("Git checkout is unavailable")?;
                        // Fetching immutable objects is safe while local edits wait for recovery.
                        if repo.head_ref().as_deref() != Some(remote_branch.as_str()) {
                            bail!("Git commits differ between branches");
                        }

                        let need = repo.head().is_some_and(|oid| {
                            repo.is_ancestor(target, oid).is_none()
                                && repo.is_ancestor(oid, target).is_none()
                        });
                        Ok((need, repo.haves(&peer_id, &remote_branch)))
                    })
                    .await??;

                    if need {
                        receive_objects(
                            connection,
                            shared,
                            ObjectWant {
                                commit: target,
                                branch: branch.clone().unwrap_or_default(),
                                haves,
                                scope: ObjectScope::Checkout,
                            },
                        )
                        .await?;
                    }
                }
                shared.first_contact(&peer.id, checkout, branch).await?;
                last = behind;

                if last.is_none() {
                    caught_up(shared, &peer.id);
                }
            }
            Reply::Changes {
                replica,
                entries,
                upto,
                checkout,
            } => {
                if shared.head().await?.differs(checkout)
                    && !shared.copies_git()
                    && !shared.branch_aware(&peer.id)
                {
                    bail!("Git commits differ; waiting for Git before accepting this batch");
                }

                if !shared.copies_git() {
                    shared.check_git().await?;
                }

                shared.merge_from(&peer.id, &entries, Cursor { replica, seq: upto });

                if last.is_some_and(|last| upto >= last) {
                    last = None;
                    caught_up(shared, &peer.id);
                }
            }
            Reply::Refused { message } => bail!("the peer refused: {message}"),
            Reply::Handoff { .. } | Reply::GitChunk { .. } | Reply::GitDone => {
                bail!("the peer answered another request")
            }
        }
    }

    Ok(())
}

/// The pull brought all `peer` had.
fn caught_up(shared: &Shared, peer: &EndpointId) {
    shared.heard(peer);
    shared.caught_up(peer);
}

#[cfg(test)]
#[path = "../../tests/files_sync_pool.rs"]
mod tests;
