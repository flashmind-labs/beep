//! Fetching blobs from peers, and copying them out of the store.

use super::{shared::Shared, task::RETRY};
use crate::files::sync::{Parts, map::FileMeta, scan::MAX_BYTES};
use crate::files::transfer::place::{Reserved, space_at};
use crate::{AsyncMutex, lock};
use anyhow::{Context, Result, anyhow, bail};
use bao_tree::io::BaoContentItem;
use futures::StreamExt as _;
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use iroh_blobs::{
    Hash, HashAndFormat,
    api::{Store, TempTag, remote::GetProgressItem},
    get::request::{GetBlobItem, get_blob},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::SeekFrom,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    fs::File as AsyncFile,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    task::{JoinSet, spawn_blocking},
    time::{Instant, timeout},
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
/// How many blobs a round fetches at once over a peer's connection, which
/// takes 100 streams. Each one that is not small holds three files open.
const MAX_FETCHES: usize = 16;

/// The peers to ask for a file, in order. A peer that could not be dialled
/// comes last, and not at all within `RETRY` of that: each dial of a machine
/// that is off takes `CONNECT_TIMEOUT`, which every file would otherwise wait.
pub(super) fn peers_to_ask(
    peers: &[EndpointAddr],
    down: &HashMap<EndpointId, Instant>,
    now: Instant,
) -> Vec<EndpointAddr> {
    let mut ask: Vec<EndpointAddr> = peers
        .iter()
        .filter(|peer| {
            down.get(&peer.id)
                .is_none_or(|at| now.saturating_duration_since(*at) >= RETRY)
        })
        .cloned()
        .collect();
    ask.sort_by_key(|peer| down.contains_key(&peer.id));
    ask
}

/// A fetched blob, kept and with its disk space held until written.
pub(super) struct Fetched {
    _tag: TempTag,
    _room: Reserved,
}

/// What a fetch gives: nothing to hold when the store had the blob already.
type Got = Result<Option<Fetched>>;

/// What fetches a project's blobs: one connection per peer, any number of
/// fetches at once. Shared with the fetches `Ahead` runs in their own tasks.
pub(super) struct Fetcher {
    pub(super) shared: Arc<Shared>,
    pub(super) parts: Parts,
    /// Locked across a dial, so fetches that start together share one.
    blobs: AsyncMutex<HashMap<EndpointId, Connection>>,
    /// When a dial to a peer last failed, until one succeeds.
    down: Mutex<HashMap<EndpointId, Instant>>,
    /// Tags on what fetches got so far, from the moment one starts: without
    /// one, the store's collector deletes it between rounds, or after a fetch
    /// dropped on a pause, and the file starts over.
    unfinished: Mutex<HashMap<Hash, TempTag>>,
}

impl Fetcher {
    pub(super) fn new(shared: Arc<Shared>, parts: Parts) -> Self {
        Self {
            shared,
            parts,
            blobs: AsyncMutex::default(),
            down: Mutex::default(),
            unfinished: Mutex::default(),
        }
    }

    /// Forgets what came of the blobs the list no longer wants.
    pub(super) fn keep_only(&self, wanted: &HashSet<Hash>) {
        lock(&self.unfinished).retain(|hash, _| wanted.contains(hash));
    }

    /// Makes sure the store holds `file`'s blob.
    async fn get(&self, path: &str, file: &FileMeta) -> Got {
        let hash = Hash::from_bytes(file.hash);
        if self.parts.store.blobs().has(hash).await? {
            return Ok(None);
        }
        self.fetch(path, file).await.map(Some)
    }

    /// Fetches `file`'s blob from the first peer that has it.
    async fn fetch(&self, path: &str, file: &FileMeta) -> Result<Fetched> {
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
        if !lock(&self.unfinished).contains_key(&content.hash) {
            let tag = self.parts.store.tags().temp_tag(content).await?;
            lock(&self.unfinished).insert(content.hash, tag);
        }
        let peers = {
            let down = lock(&self.down);
            peers_to_ask(&self.shared.peers.borrow(), &down, Instant::now())
        };
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
                    let tag = lock(&self.unfinished).remove(&content.hash);
                    return Ok(Fetched {
                        _tag: tag.context("the file stopped being wanted")?,
                        _room: room,
                    });
                }
                Ok(()) => last = Some(anyhow!("{} sent only part of it", peer.id)),
                Err(e) => {
                    if self.shared.stop.is_cancelled() {
                        return Err(e);
                    }
                    self.drop_connection(&peer.id, &connection).await;
                    last = Some(e.context(peer.id.to_string()));
                }
            }
        }
        Err(match last {
            Some(e) => e.context(format!("no machine that syncs it could send {path} now")),
            None => anyhow!("no other machine that syncs {path} can be reached now"),
        })
    }

    /// Writes blob `hash` into `out` from the first peer that sends all of
    /// it, verified as it comes, without the store.
    pub(super) async fn copy_from_peers(&self, hash: Hash, out: &mut AsyncFile) -> Result<()> {
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
                    self.blobs.lock().await.remove(&peer.id);
                    last = e.context(format!("{}", peer.id));
                }
            }
        }
        Err(last.context("no machine could send it again"))
    }

    /// Stops using `connection`, and closes it for the fetches still on it.
    async fn drop_connection(&self, peer: &EndpointId, connection: &Connection) {
        let mut blobs = self.blobs.lock().await;
        if blobs
            .get(peer)
            .is_some_and(|open| open.stable_id() == connection.stable_id())
        {
            blobs.remove(peer);
        }
        connection.close(0u32.into(), b"fetch failed");
    }

    async fn blobs_to(&self, peer: &EndpointAddr) -> Result<Connection> {
        let mut blobs = self.blobs.lock().await;
        if let Some(connection) = blobs.get(&peer.id)
            && connection.close_reason().is_none()
        {
            return Ok(connection.clone());
        }
        let dialled = timeout(
            CONNECT_TIMEOUT,
            self.parts.endpoint.connect(peer.clone(), iroh_blobs::ALPN),
        )
        .await
        .map_err(|_| anyhow!("could not reach {} in time", peer.id))
        .and_then(|connected| connected.map_err(anyhow::Error::from));
        let connection = match dialled {
            Ok(connection) => connection,
            Err(e) => {
                lock(&self.down).insert(peer.id, Instant::now());
                return Err(e);
            }
        };
        lock(&self.down).remove(&peer.id);
        blobs.insert(peer.id, connection.clone());
        Ok(connection)
    }
}

/// A file a round will write, whose blob may need fetching.
pub(super) struct Wanted {
    pub(super) path: String,
    pub(super) file: FileMeta,
}

/// The blobs a round needs next, fetched ahead of the files being written,
/// at most `MAX_FETCHES` running or done and not yet taken. They run in their
/// own tasks, to go on while the round writes; dropping this stops them.
pub(super) struct Ahead {
    fetcher: Arc<Fetcher>,
    wanted: VecDeque<Wanted>,
    started: HashSet<Hash>,
    running: JoinSet<(Hash, Got)>,
    done: HashMap<Hash, Got>,
}

impl Ahead {
    pub(super) fn new(fetcher: Arc<Fetcher>, wanted: VecDeque<Wanted>) -> Self {
        Self {
            fetcher,
            wanted,
            started: HashSet::new(),
            running: JoinSet::new(),
            done: HashMap::new(),
        }
    }

    fn fill(&mut self) {
        while self.running.len() + self.done.len() < MAX_FETCHES {
            let Some(next) = self.wanted.pop_front() else {
                return;
            };
            let hash = Hash::from_bytes(next.file.hash);
            if !self.started.insert(hash) {
                continue;
            }
            let fetcher = self.fetcher.clone();
            self.running
                .spawn(async move { (hash, fetcher.get(&next.path, &next.file).await) });
        }
    }

    /// What came of `hash`, if that is known yet. With nothing left running
    /// it is `Ok(None)`: a blob two paths share is fetched once and the later
    /// path finds it in the store, or else `copy_out` asks a peer for it.
    pub(super) fn take(&mut self, hash: &Hash) -> Option<Got> {
        self.fill();
        match self.done.remove(hash) {
            Some(got) => Some(got),
            None if self.running.is_empty() => Some(Ok(None)),
            None => None,
        }
    }

    /// Lets go of what came for a file the round did not write.
    pub(super) fn forget(&mut self, hash: &Hash) {
        self.done.remove(hash);
    }

    /// Waits for one more fetch to finish.
    pub(super) async fn finish_one(&mut self) {
        match self.running.join_next().await {
            Some(Ok((hash, got))) => {
                self.done.insert(hash, got);
            }
            Some(Err(e)) => log::warn!("a fetch task failed: {e}"),
            None => std::future::pending().await,
        }
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

/// Reads blob `hash` from the store, at most `limit` bytes of it.
pub(super) async fn read_store(store: &Store, hash: Hash, limit: u64) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    store
        .blobs()
        .reader(hash)
        .take(limit + 1)
        .read_to_end(&mut raw)
        .await
        .context("reading the store")?;
    if raw.len() as u64 > limit {
        bail!("the file is larger than {limit} bytes");
    }
    if *blake3::hash(&raw).as_bytes() != *hash.as_bytes() {
        bail!("the copy in the store changed");
    }
    Ok(raw)
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
