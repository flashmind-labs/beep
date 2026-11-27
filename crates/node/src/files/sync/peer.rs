//! Node-to-node sync on `ALPN`: `pull` reads a peer's project lists directly
//! over iroh, so sync goes on while the server is down, and `PeerHandler`
//! serves them to each project's peers only.

use super::{
    Syncs,
    gitdir::{Checkout, Repo},
    project::{Cursor, Shared},
};
use crate::{AsyncMutex, lock, membership::Membership};
use anyhow::{Context, Result, bail};
use beep_mesh::{OPEN_TIMEOUT, RETRY};
use beep_proto::node::{LinkRx, LinkTx, link};
use beep_proto::watched;
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::{
    collections::HashMap,
    fmt,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, timeout},
};
use tracing::Instrument;
use wire::{ObjectScope, ObjectWant, Reply, Request, receive_objects};

mod branches;
mod wire;

/// Every batch carries its scanned checkout. Earlier versions cannot
/// enforce that base when Git changes during a pull.
pub(crate) const ALPN: &[u8] = b"beep-sync/7";

/// The most entries one batch carries.
const BATCH: usize = 1000;
/// The most projects one peer pulls at once over a shared connection.
const MAX_STREAMS: usize = 256;
/// The most active pulls placed onto a single pooled connection. Stays under
/// released peers' `MAX_STREAMS = 64` and Quinn's 100-bidi-stream limit so
/// auxiliary fetch streams (`GetGit`/`GetObjects`/`Get`) always have headroom.
const POOL_MAX_PULLS: usize = 48;
/// The most commits a puller offers as had.
const MAX_HAVES: usize = 64;
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
        self.pool.mark_broken(self.peer, &self.conn);
    }
}

impl Drop for SyncLease {
    fn drop(&mut self) {
        let close = {
            let mut peers = lock(&self.pool.peers);
            let mut close = None;

            if let Some(list) = peers.get_mut(&self.peer) {
                if let Some(index) = list
                    .iter()
                    .position(|held| held.conn.stable_id() == self.conn.stable_id())
                {
                    list[index].leases = list[index].leases.saturating_sub(1);

                    if list[index].leases == 0 {
                        close = Some(list.remove(index).conn);
                    }
                }

                if list.is_empty() {
                    peers.remove(&self.peer);
                }
            }

            close
        };

        if let Some(conn) = close {
            conn.close(0u32.into(), b"idle");
        }
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

    fn mark_broken(&self, peer: EndpointId, connection: &Connection) {
        let close = {
            let mut peers = lock(&self.peers);
            let mut close = None;

            if let Some(list) = peers.get_mut(&peer) {
                if let Some(index) = list
                    .iter()
                    .position(|held| held.conn.stable_id() == connection.stable_id())
                {
                    list[index].broken = true;

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
            conn.close(0u32.into(), b"broken");
        }
    }
}

/// Answers other nodes' pulls.
#[derive(Clone)]
pub(crate) struct PeerHandler {
    pub(crate) syncs: Syncs,
}

impl fmt::Debug for PeerHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerHandler").finish()
    }
}

impl ProtocolHandler for PeerHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let peer = connection.remote_id();

        if !self.syncs.is_peer(&peer) {
            log::info!("refused a sync connection from {peer}: not a peer");
            connection.close(1u32.into(), b"not a peer");
            return Ok(());
        }
        // The streams end with the connection: dropping the set stops them.
        let mut streams = JoinSet::new();

        loop {
            tokio::select! {
                stream = connection.accept_bi() => {
                    let Ok((send, recv)) = stream else {
                        return Ok(());
                    };

                    if streams.len() >= MAX_STREAMS {
                        log::warn!("refused a pull from {peer}: {MAX_STREAMS} open");
                        continue;
                    }

                    let (tx, rx) = link(send, recv);
                    streams.spawn(serve(self.syncs.clone(), peer, tx, rx).in_current_span());
                }
                Some(ended) = streams.join_next() => {
                    match ended {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => log::debug!("a pull from {peer} ended: {e:#}"),
                        Err(e) => log::warn!("a pull task failed: {e}"),
                    }

                    if !self.syncs.is_peer(&peer) {
                        log::info!("closed the sync connection from {peer}: no longer a peer");
                        connection.close(1u32.into(), b"not a peer");
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn serve(syncs: Syncs, peer: iroh::EndpointId, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let mut with_branches = false;
    let (project, replica, since, copy_git) = match rx.recv::<Request>().await? {
        Some(Request::Pull {
            project,
            replica,
            since,
            copy_git,
        }) => (project, replica, since, copy_git),
        Some(Request::PullBranches {
            project,
            replica,
            since,
            copy_git,
        }) => {
            with_branches = true;
            (project, replica, since, copy_git)
        }
        Some(Request::GitObjects {
            project,
            commit,
            branch,
            have,
            haves,
        }) => {
            return serve_objects(
                &syncs,
                peer,
                &project,
                &mut tx,
                ObjectWant {
                    commit,
                    branch,
                    haves: have.into_iter().chain(haves).collect(),
                    scope: ObjectScope::Checkout,
                },
            )
            .await;
        }
        Some(Request::BranchObjects {
            project,
            commit,
            branch,
            have,
            haves,
        }) => {
            return serve_objects(
                &syncs,
                peer,
                &project,
                &mut tx,
                ObjectWant {
                    commit,
                    branch,
                    haves: have.into_iter().chain(haves).collect(),
                    scope: ObjectScope::Branch,
                },
            )
            .await;
        }
        Some(Request::Handoff { project }) => {
            if !syncs
                .project(&project)
                .is_some_and(|shared| shared.has_peer(&peer))
                || !syncs.allows(&peer)
            {
                return tx
                    .send(&Reply::Refused {
                        message: "This project is not shared with this peer.".into(),
                    })
                    .await;
            }

            let reply = match syncs.handoff_snapshot(&project).await {
                Ok(snapshot) => Reply::Handoff { snapshot },
                Err(e) => Reply::Refused {
                    message: format!("{e:#}"),
                },
            };
            return tx.send(&reply).await;
        }
        None => return Ok(()),
    };
    let Some(shared) = syncs
        .project(&project)
        .filter(|shared| shared.has_peer(&peer) && syncs.allows(&peer))
    else {
        let message = format!("{project} is not synced with this node");
        return tx.send(&Reply::Refused { message }).await;
    };
    let _serving = syncs.serving();
    let mut peers = shared.watch_peers();

    loop {
        tokio::select! {
            scanned = shared.scan_before_matching() => {
                scanned?;
                break;
            }
            () = shared.stopped() => return Ok(()),
            // A node taken out of the project gets nothing more.
            changed = peers.changed() => if changed.is_err() || !shared.has_peer(&peer) || !syncs.allows(&peer) {
                return Ok(());
            },
        }
    }

    let mut since = if replica == shared.replica() {
        since
    } else {
        0
    };
    let mut head = shared.list_checkout().await?;

    if shared.copies_git() && head.present {
        return tx
            .send(&Reply::Refused {
                message: "The initial Git checkout is still arriving.".into(),
            })
            .await;
    }

    let mut branch = git_branch(shared.root()).await?;
    let mut tips = if with_branches {
        let tips = branches::catalog(&shared).await?;
        tx.send(&Reply::Branches { tips: tips.clone() }).await?;
        tips
    } else {
        Vec::new()
    };
    tx.send(&Reply::Head {
        commit: head.commit,
        behind: shared.last_change_after(since),
        has_git: head.present,
        branch: branch.clone(),
    })
    .await?;
    let mut seq = shared.subscribe();
    let mut saved = shared.watch_saved();

    loop {
        // A device removed meanwhile gets nothing more, not even what is
        // already queued.
        if !syncs.allows(&peer) || shared.is_stopped() {
            return Ok(());
        }

        let now = match shared.list_checkout().await {
            Ok(head) => head,
            Err(_) => {
                shared.scan_before_matching().await?;
                shared.list_checkout().await?
            }
        };

        if with_branches {
            let next = branches::catalog(&shared).await?;

            if next != tips {
                tx.send(&Reply::Branches { tips: next.clone() }).await?;
                tips = next;
            }
        }

        let next_branch = git_branch(shared.root()).await?;

        if now != head || next_branch != branch {
            shared.scan_before_matching().await?;
            head = shared.head().await?;
            branch = next_branch;
            tx.send(&Reply::Head {
                commit: head.commit,
                behind: shared.last_change_after(since),
                has_git: head.present,
                branch: branch.clone(),
            })
            .await?;
        }

        let mut changes = shared.changes(since, BATCH, head)?;

        if !changes.entries.is_empty() {
            since = changes.upto;
            let git_only = shared.status().git_only;
            changes.entries.retain(|entry| {
                let git = super::gitdir::in_git(&entry.path);
                if git_only {
                    copy_git && head.present && git && shared.may_export(&entry.path)
                } else {
                    !git
                }
            });
            tx.send(&Reply::Changes {
                replica: shared.replica(),
                entries: changes.entries,
                upto: changes.upto,
                checkout: head,
            })
            .await?;
            continue;
        }

        tokio::select! {
            changed = seq.changed() => if changed.is_err() {
                return Ok(());
            },
            changed = saved.changed() => if changed.is_err() {
                return Ok(());
            },
            () = shared.stopped() => return Ok(()),
            // A node taken out of the project gets nothing more.
            changed = peers.changed() => if changed.is_err() || !shared.has_peer(&peer) || !syncs.allows(&peer) {
                return Ok(());
            },
            // The peer sends nothing more; it closing ends the pull.
            _ = rx.recv::<Request>() => return Ok(()),
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

/// The branch is checked separately from the object ID before moving a checkout.
async fn git_branch(root: &std::path::Path) -> Result<Option<String>> {
    let root = root.to_owned();
    Ok(spawn_blocking(move || Repo::open(&root).and_then(|repo| repo.head_ref())).await?)
}

async fn serve_objects(
    syncs: &Syncs,
    peer: iroh::EndpointId,
    project: &str,
    tx: &mut LinkTx,
    want: ObjectWant,
) -> Result<()> {
    let Some(shared) = syncs
        .project(project)
        .filter(|shared| shared.has_peer(&peer) && syncs.allows(&peer))
    else {
        return tx
            .send(&Reply::Refused {
                message: "This project is not shared with this peer.".into(),
            })
            .await;
    };
    let ObjectWant {
        commit,
        branch,
        haves,
        scope,
    } = want;
    let root = shared.root().to_owned();
    let (command, input) = spawn_blocking(move || -> Result<_> {
        let repo = Repo::open(&root).context("Git checkout is unavailable")?;

        match scope {
            ObjectScope::Checkout => {
                if repo.head() != Some(commit)
                    || repo.head_ref().as_deref() != Some(branch.as_str())
                {
                    bail!("The peer checkout changed; retrying sync");
                }
            }
            ObjectScope::Branch => {
                repo.validate_branch(&branch)?;

                if repo.ref_oid(&branch) != Some(commit) {
                    bail!("The peer branch changed; retrying sync");
                }
            }
        }

        // Every commit the puller has that is here too stays out of the
        // pack, related to `commit` or not, and deltas may build on them.
        let mut input = format!("{commit}\n");

        for have in haves.into_iter().take(MAX_HAVES) {
            if repo.has_commit(have) {
                input.push_str(&format!("^{have}\n"));
            }
        }

        let mut command = repo.git();
        command
            .args(["pack-objects", "--stdout", "--revs", "--thin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        Ok((command, input))
    })
    .await??;
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().context("pack input missing")?;
    stdin.write_all(input.as_bytes()).await?;
    drop(stdin);
    let mut stdout = child.stdout.take().context("pack output missing")?;
    let mut buffer = vec![0; 64 * 1024];

    loop {
        let n = stdout.read(&mut buffer).await?;

        if n == 0 {
            break;
        }

        if !syncs.allows(&peer) || !shared.has_peer(&peer) || shared.is_stopped() {
            bail!("Project sharing stopped");
        }

        tx.send(&Reply::GitChunk {
            bytes: buffer[..n].to_vec(),
        })
        .await?;
    }

    if !child.wait().await?.success() {
        bail!("Git could not create the object pack");
    }

    tx.send(&Reply::GitDone).await
}
