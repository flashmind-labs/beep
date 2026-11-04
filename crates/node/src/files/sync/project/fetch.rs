//! Fetching blobs from peers, and copying them out of the store.

use super::task::Task;
use crate::files::sync::{map::FileMeta, scan::MAX_BYTES};
use crate::files::transfer::place::{Reserved, space_at};
use anyhow::{Context, Result, anyhow, bail};
use bao_tree::io::BaoContentItem;
use futures::StreamExt as _;
use iroh::{EndpointAddr, endpoint::Connection};
use iroh_blobs::{
    Hash, HashAndFormat,
    api::{Store, TempTag, remote::GetProgressItem},
    get::request::{GetBlobItem, get_blob},
};
use std::{io::SeekFrom, time::Duration};
use tokio::{
    fs::File as AsyncFile,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    task::spawn_blocking,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a fetch may wait for its next data before it gives up. A
/// connection can stay open on a path that no longer carries data (a NAT
/// mapping gone, with the relay keeping the connection alive); without a
/// limit such a fetch waits forever. The next round fetches again over a
/// new connection and keeps what came.
const STALL: Duration = Duration::from_secs(30);
/// What every disk keeps free beyond a fetch.
const SPACE_MARGIN: u64 = 64 << 20;
const CHUNK: usize = 1 << 20;

/// A fetched blob, kept and with its disk space held until written.
pub(super) struct Fetched {
    _tag: TempTag,
    _room: Reserved,
}

impl Task {
    /// Fetches `file`'s blob from the first peer that has it.
    pub(super) async fn fetch(&mut self, path: &str, file: &FileMeta) -> Result<Fetched> {
        if file.size > MAX_BYTES {
            bail!("the file is larger than {} GiB", MAX_BYTES >> 30);
        }
        let room = {
            let (store_dir, root, room) = (
                self.parts.store_dir.clone(),
                self.shared.root.clone(),
                self.parts.room.clone(),
            );
            let need = file.size;
            spawn_blocking(move || {
                room.reserve(need, space_at(&store_dir)?, root.space()?, SPACE_MARGIN)
            })
            .await??
        };
        let content = HashAndFormat::raw(Hash::from_bytes(file.hash));
        let tag = self.parts.store.tags().temp_tag(content).await?;
        let peers = self.shared.peers.borrow().clone();
        let mut last = None;
        for peer in peers {
            let connection = match self.blobs_to(&peer).await {
                Ok(connection) => connection,
                Err(e) => {
                    last = Some(e);
                    continue;
                }
            };
            let fetched = fetch_within(
                &self.parts.store,
                connection.clone(),
                content,
                STALL,
                &self.shared.stop,
            )
            .await;
            match fetched {
                Ok(()) if self.parts.store.blobs().has(content.hash).await? => {
                    return Ok(Fetched {
                        _tag: tag,
                        _room: room,
                    });
                }
                Ok(()) => last = Some(anyhow!("{} sent only part of it", peer.id)),
                Err(e) => {
                    if self.shared.stop.is_cancelled() {
                        return Err(e);
                    }
                    self.blobs.remove(&peer.id);
                    connection.close(0u32.into(), b"fetch failed");
                    last = Some(e.context(peer.id.to_string()));
                }
            }
        }
        Err(match last {
            Some(e) => e.context(format!("no machine that syncs it could send {path} now")),
            None => anyhow!("no other machine syncs {path}"),
        })
    }

    /// Writes blob `hash` into `out` from the first peer that sends all of
    /// it, verified as it comes, without the store.
    pub(super) async fn copy_from_peers(&mut self, hash: Hash, out: &mut AsyncFile) -> Result<()> {
        let peers = self.shared.peers.borrow().clone();
        let mut last = anyhow!("no other machine syncs it");
        for peer in peers {
            out.set_len(0).await?;
            out.seek(SeekFrom::Start(0)).await?;
            let copied = match self.blobs_to(&peer).await {
                Ok(connection) => copy_blob(connection, hash, out, &self.shared.stop).await,
                Err(e) => Err(e),
            };
            match copied {
                Ok(copied) if copied == *hash.as_bytes() => return Ok(()),
                Ok(_) => last = anyhow!("{} sent other content", peer.id),
                Err(e) => {
                    self.blobs.remove(&peer.id);
                    last = e.context(format!("{}", peer.id));
                }
            }
        }
        Err(last.context("no machine could send it again"))
    }

    async fn blobs_to(&mut self, peer: &EndpointAddr) -> Result<Connection> {
        if let Some(connection) = self.blobs.get(&peer.id)
            && connection.close_reason().is_none()
        {
            return Ok(connection.clone());
        }
        let connection = timeout(
            CONNECT_TIMEOUT,
            self.parts.endpoint.connect(peer.clone(), iroh_blobs::ALPN),
        )
        .await
        .map_err(|_| anyhow!("could not reach {} in time", peer.id))??;
        self.blobs.insert(peer.id, connection.clone());
        Ok(connection)
    }
}

/// Writes blob `hash` into `out` from the store, or with `refetch` when
/// the store's copy cannot be read or is not that content any more: the
/// store reads it from a file in place, which may have changed or gone
/// since. A peer sends it again, checked as it comes.
pub(super) async fn copy_out(
    store: &Store,
    hash: Hash,
    out: &mut AsyncFile,
    stop: &CancellationToken,
    refetch: impl AsyncFnOnce(&mut AsyncFile) -> Result<()>,
) -> Result<()> {
    match copy_store(store, hash, out, stop).await {
        Ok(copied) if copied == *hash.as_bytes() => return Ok(()),
        Ok(_) => {}
        Err(_) if stop.is_cancelled() => bail!("stopped"),
        Err(e) => log::debug!("reading {hash} from the store: {e:#}"),
    }
    refetch(out).await
}

/// Copies blob `hash` from the store into `out`, giving the hash of what
/// was written: another hash when the file the store reads it from was
/// changed in place.
pub(super) async fn copy_store(
    store: &Store,
    hash: Hash,
    out: &mut AsyncFile,
    stop: &CancellationToken,
) -> Result<[u8; 32]> {
    let mut reader = store.blobs().reader(hash);
    let mut buffer = vec![0; CHUNK];
    let mut hasher = blake3::Hasher::new();
    loop {
        if stop.is_cancelled() {
            bail!("stopped");
        }
        let read = reader
            .read(&mut buffer)
            .await
            .context("reading the store")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        out.write_all(&buffer[..read]).await?;
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Copies blob `hash` from a peer into `out`, giving the hash of what was
/// written. The blobs protocol checks every chunk against `hash`.
/// Fetches `content` into `store` over `connection`, failing when no data
/// comes for `stall`.
pub(super) async fn fetch_within(
    store: &Store,
    connection: Connection,
    content: HashAndFormat,
    stall: Duration,
    stop: &CancellationToken,
) -> Result<()> {
    let items = store.remote().fetch(connection, content).stream();
    let mut items = std::pin::pin!(items);
    loop {
        let item = tokio::select! {
            item = timeout(stall, items.next()) => item
                .map_err(|_| anyhow!("no data came for {} s", stall.as_secs_f32()))?,
            () = stop.cancelled() => bail!("stopped"),
        };
        match item {
            Some(GetProgressItem::Progress(_)) => {}
            Some(GetProgressItem::Done(_)) => return Ok(()),
            Some(GetProgressItem::Error(e)) => bail!("{e}"),
            None => bail!("the fetch ended early"),
        }
    }
}

async fn copy_blob(
    connection: Connection,
    hash: Hash,
    out: &mut AsyncFile,
    stop: &CancellationToken,
) -> Result<[u8; 32]> {
    let mut items = get_blob(connection, hash);
    let mut hasher = blake3::Hasher::new();
    let mut at = 0u64;
    loop {
        let item = tokio::select! {
            item = timeout(STALL, items.next()) => item
                .map_err(|_| anyhow!("no data came for {} s", STALL.as_secs()))?,
            () = stop.cancelled() => bail!("stopped"),
        };
        match item {
            Some(GetBlobItem::Item(BaoContentItem::Leaf(leaf))) => {
                if leaf.offset != at {
                    bail!("the blob came out of order");
                }
                hasher.update(&leaf.data);
                out.write_all(&leaf.data).await?;
                at += leaf.data.len() as u64;
            }
            Some(GetBlobItem::Item(BaoContentItem::Parent(_))) => {}
            Some(GetBlobItem::Done(_)) => return Ok(*hasher.finalize().as_bytes()),
            Some(GetBlobItem::Error(e)) => return Err(e.into()),
            None => bail!("the blob ended early"),
        }
    }
}
