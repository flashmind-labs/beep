//! Node-to-node sync on `ALPN`: `pull` reads a peer's project lists directly
//! over iroh, so sync goes on while the server is down, and `PeerHandler`
//! serves them to each project's peers only.

use tracing::Instrument;
use super::{
    CONNECT_TIMEOUT, STALL, Syncs,
    gitdir::{BranchTip, Checkout, Oid, Repo},
    map::{Entry, Replica},
    project::{Cursor, Shared},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{LinkRx, LinkTx, link};
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use std::{fmt, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, timeout},
};

mod branches;

/// Every batch carries its scanned checkout. Earlier versions cannot
/// enforce that base when Git changes during a pull.
pub(crate) const ALPN: &[u8] = b"beep-sync/7";

/// The most entries one batch carries.
const BATCH: usize = 1000;
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);
/// The most projects one peer pulls at once over a connection.
const MAX_STREAMS: usize = 64;
/// How long a Git object transfer waits for its first chunk, then for each
/// next one.
const FIRST_CHUNK: Duration = Duration::from_secs(300);
/// The largest Git pack one transfer takes.
const MAX_PACK: u64 = 2 << 30;
/// The most commits a puller offers as had.
const MAX_HAVES: usize = 64;

/// Sent as compact msgpack, like the node link, so a field is added only at
/// the end with `#[serde(default)]`. The same goes for `Reply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
enum Request {
    Handoff {
        project: String,
    },
    /// Sends `project`'s changes after `since`, when `replica` is the
    /// peer's replica, or all of them, then each change as it happens.
    Pull {
        project: String,
        replica: Replica,
        since: u64,
        #[serde(default)]
        copy_git: bool,
    },
    GitObjects {
        project: String,
        commit: Oid,
        branch: String,
        have: Option<Oid>,
        /// More commits the puller has, beyond `have`, for the pack to leave out.
        #[serde(default)]
        haves: Vec<Oid>,
    },
    /// The branch catalogue reply is sent only in response to this request.
    PullBranches {
        project: String,
        replica: Replica,
        since: u64,
        copy_git: bool,
    },
    BranchObjects {
        project: String,
        commit: Oid,
        branch: String,
        have: Option<Oid>,
        #[serde(default)]
        haves: Vec<Oid>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
enum Reply {
    Handoff {
        snapshot: beep_proto::node::sync::HandoffSnapshot,
    },
    /// Sent first: the commit the peer's checkout is at, if it has one.
    Head {
        commit: Option<Oid>,
        /// The peer's last change the puller lacks, if any: the puller has
        /// all the peer had once its changes reach it. Peers before this
        /// field leave it out.
        #[serde(default)]
        behind: Option<u64>,
        #[serde(default)]
        has_git: bool,
        #[serde(default)]
        branch: Option<String>,
    },
    /// Changes of `replica` up to its change `upto`.
    Changes {
        replica: Replica,
        entries: Vec<Entry>,
        upto: u64,
        checkout: Checkout,
    },
    Refused {
        message: String,
    },
    GitChunk {
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    GitDone,
    Branches {
        tips: Vec<BranchTip>,
    },
}

#[derive(Clone, Copy)]
enum ObjectScope {
    Checkout,
    Branch,
}

struct ObjectWant {
    commit: Oid,
    branch: String,
    /// Commits the puller has, the closest first.
    haves: Vec<Oid>,
    scope: ObjectScope,
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

/// Pulls `shared`'s list from `peer` until the project stops, reconnecting
/// with growing waits. Only a pull that brought all the peer had starts
/// the waits over; one that fails later, as on a Git wait, keeps growing
/// them. Asked to pull again, a waiting pull dials at once.
pub(super) async fn pull(shared: Arc<Shared>, endpoint: Endpoint, peer: EndpointAddr) {
    let mut wait = FIRST_RETRY;
    let mut again = shared.watch_pull_again();

    loop {
        again.mark_unchanged();
        let pulled = tokio::select! {
            pulled = pull_once(&shared, &endpoint, &peer, &mut wait) => pulled,
            () = shared.stopped() => return,
        };

        if let Err(e) = pulled {
            log::debug!("pulling {} from {}: {e:#}", shared.name(), peer.id);
        }

        shared.heard(&peer.id);
        tokio::select! {
            () = sleep(wait) => {}
            _ = again.changed() => {}
            () = shared.stopped() => return,
        }
        wait = (wait * 2).min(LAST_RETRY);
    }
}

async fn pull_once(
    shared: &Shared,
    endpoint: &Endpoint,
    peer: &EndpointAddr,
    wait: &mut Duration,
) -> Result<()> {
    let started = Instant::now();
    let connection = timeout(CONNECT_TIMEOUT, endpoint.connect(peer.clone(), ALPN))
        .await
        .context("could not reach the peer in time")??;
    let (_tx, mut rx, mut first) = branches::open_pull(&connection, shared, &peer.id).await?;
    let mut last = None;
    let mut again = shared.watch_pull_again();
    again.mark_unchanged();

    loop {
        let reply = if let Some(first) = first.take() {
            Some(first)
        } else {
            tokio::select! {
                reply = rx.recv::<Reply>() => reply?,
                // Asked to start over: dropping the connection ends the pull.
                _ = again.changed() => return Ok(()),
            }
        };
        let Some(reply) = reply else {
            break;
        };

        match reply {
            Reply::Branches { tips } => {
                branches::receive_catalog(&connection, shared, &peer.id, tips).await?;
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
                            &connection,
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
                    caught_up(shared, &peer.id, wait);
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
                    caught_up(shared, &peer.id, wait);
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

/// The pull brought all `peer` had: it went through, so losing the
/// connection now is no reason to wait long.
fn caught_up(shared: &Shared, peer: &EndpointId, wait: &mut Duration) {
    shared.heard(peer);
    shared.caught_up(peer);
    *wait = FIRST_RETRY;
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

async fn receive_objects(connection: &Connection, shared: &Shared, want: ObjectWant) -> Result<()> {
    let (send, recv) = timeout(CONNECT_TIMEOUT, connection.open_bi())
        .await
        .context("The Git object transfer did not start")??;
    let (mut tx, mut rx) = link(send, recv);
    let ObjectWant {
        commit,
        branch,
        haves,
        scope,
    } = want;
    // Peers before `haves` read only `have`: the closest commit.
    let have = haves.first().copied();
    let request = match scope {
        ObjectScope::Checkout => Request::GitObjects {
            project: shared.name().to_owned(),
            commit,
            branch,
            have,
            haves,
        },
        ObjectScope::Branch => Request::BranchObjects {
            project: shared.name().to_owned(),
            commit,
            branch,
            have,
            haves,
        },
    };
    tx.send(&request).await?;

    let root = shared.root().to_owned();
    let (repo, pack) = spawn_blocking(move || -> Result<_> {
        let repo = Repo::open(&root).context("Git checkout is unavailable")?;
        let pack = repo.incoming_pack()?;
        Ok((repo, pack))
    })
    .await??;
    let mut file = tokio::fs::File::create(pack.received()).await?;
    let mut total = 0u64;

    loop {
        // Git writes nothing until it has counted and compressed the
        // objects, which takes long for a big pack.
        let wait = if total == 0 { FIRST_CHUNK } else { STALL };
        let reply = timeout(wait, rx.recv::<Reply>())
            .await
            .context("The Git object transfer stalled")??;

        match reply {
            Some(Reply::GitChunk { bytes }) => {
                total = total
                    .checked_add(bytes.len() as u64)
                    .context("Git pack too large")?;
                if total > MAX_PACK {
                    bail!("Git pack exceeds the transfer limit");
                }

                file.write_all(&bytes).await?;
            }
            Some(Reply::GitDone) => break,
            Some(Reply::Refused { message }) => bail!("{message}"),
            _ => bail!("The peer did not complete the Git pack"),
        }
    }

    file.flush().await?;
    drop(file);
    // Indexing runs to its end even when the pull is dropped meanwhile, so
    // Git never leaves a half-written pack of its own.
    spawn_blocking(move || repo.index_pack(&pack)).await?
}

#[cfg(test)]
#[path = "../../tests/files_sync_peer.rs"]
mod tests;
