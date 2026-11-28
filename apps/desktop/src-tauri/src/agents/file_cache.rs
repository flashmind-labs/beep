//! Media files opened from coding-agent chats, fetched from their machine
//! over iroh-blobs into a store of this app's own, so a file that did not
//! change is not downloaded again and one cut off resumes. Closing a
//! conversation drops the files only it opened; files not opened for `KEEP`
//! go, and the store stays under `MAX_BYTES`.

use crate::store::lock;
use crate::{chat::files::GONE, error::DesktopError, fleet::Fleet};
use anyhow::{Context, Result};
use beep_mesh::machine::{NodeShare, sessions};
use beep_proto::SessionId;
use futures::StreamExt;
use iroh::EndpointId;
use iroh_blobs::{
    Hash, HashAndFormat,
    api::remote::GetProgressItem,
    store::{
        GcConfig,
        fs::{FsStore, options::Options},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt},
    sync::OnceCell,
    time::timeout,
};

/// How long media copies stay after being opened.
#[cfg(desktop)]
pub(super) const KEEP: Duration = Duration::from_secs(24 * 60 * 60);
#[cfg(mobile)]
pub(super) const KEEP: Duration = Duration::from_secs(8 * 60 * 60);
/// Each media store's budget, except the copy currently being opened.
#[cfg(desktop)]
pub(super) const MAX_BYTES: u64 = 5 << 30;
#[cfg(mobile)]
pub(super) const MAX_BYTES: u64 = 500 << 20;
/// How often the store drops the blobs no tag holds.
const GC_EVERY: Duration = Duration::from_secs(60);

/// A file in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cached {
    pub(crate) hash: Hash,
    pub(crate) size: u64,
}

/// The files of the store, saved beside it.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    /// By BLAKE3 hash, in hex, which is also the tag that keeps it.
    files: HashMap<String, Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    size: u64,
    /// When it was last opened, in seconds since the epoch.
    opened: u64,
    /// The conversations that opened it; none when it was opened outside one.
    sessions: BTreeSet<SessionId>,
}

struct Opened {
    store: FsStore,
    index: Mutex<Index>,
}

pub(crate) struct FileCache {
    dir: PathBuf,
    opened: OnceCell<Opened>,
    /// The file each machine and path held when it was last asked, for
    /// the reads after the first part.
    known: Mutex<HashMap<(EndpointId, String), Cached>>,
}

impl FileCache {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            opened: OnceCell::new(),
            known: Mutex::default(),
        }
    }

    /// Where opened copies of the files go, for the apps that show them.
    pub(crate) fn copy_path(&self, cached: Cached, name: &str) -> PathBuf {
        self.dir
            .join("open")
            .join(format!("{}-{name}", cached.hash.to_hex()))
    }

    /// The media file `path` on `machine`, fetched into the store unless it
    /// holds it already, and kept for `session`. `None` when the machine
    /// cannot share it, such as a node from before sharing: read it a part
    /// at a time then. `progress` hears the bytes downloaded.
    pub(crate) async fn fetch(
        &self,
        fleet: &Fleet,
        machine: EndpointId,
        path: &str,
        session: Option<&SessionId>,
        mut progress: impl FnMut(u64, u64),
    ) -> crate::error::Result<Option<Cached>> {
        let client = fleet.client(machine).await?;
        let shared = match sessions::share(&client, path).await {
            Ok(NodeShare::Shared { hash, size }) => Cached {
                hash: Hash::from_bytes(hash),
                size,
            },
            Ok(NodeShare::Missing) => return Err(DesktopError::BadRequest(GONE.into())),
            Ok(NodeShare::Refused(why)) => {
                log::debug!("{} did not share a file: {why}", fleet.name_of(machine));
                return Ok(None);
            }
            Err(e) => {
                log::debug!("{} did not share a file: {e:#}", fleet.name_of(machine));
                return Ok(None);
            }
        };

        match self
            .store(fleet, machine, shared, session, &mut progress)
            .await
        {
            Ok(()) => {
                lock(&self.known).insert((machine, path.to_owned()), shared);
                Ok(Some(shared))
            }
            Err(e) => {
                log::warn!(
                    "fetching a file from {} failed: {e:#}",
                    fleet.name_of(machine)
                );
                Ok(None)
            }
        }
    }

    /// Keeps `shared` for `session` and downloads it unless the store has it.
    async fn store(
        &self,
        fleet: &Fleet,
        machine: EndpointId,
        shared: Cached,
        session: Option<&SessionId>,
        progress: &mut impl FnMut(u64, u64),
    ) -> Result<()> {
        let opened = self.open().await?;
        opened
            .store
            .tags()
            .set(shared.hash.to_hex(), HashAndFormat::raw(shared.hash))
            .await?;
        self.note(opened, shared, session)?;

        if !opened.store.blobs().has(shared.hash).await? {
            download(fleet, &opened.store, machine, shared, progress).await?;
        }

        progress(shared.size, shared.size);
        self.trim(opened, Some(shared.hash)).await;
        Ok(())
    }

    /// What `fetch` last found at `path` on `machine`.
    pub(crate) fn known(&self, machine: EndpointId, path: &str) -> Option<Cached> {
        lock(&self.known).get(&(machine, path.to_owned())).copied()
    }

    /// Up to `len` bytes of `cached` from `offset`.
    pub(crate) async fn read(&self, cached: Cached, offset: u64, len: usize) -> Result<Vec<u8>> {
        let opened = self.open().await?;
        let mut data = Vec::new();

        if offset < cached.size {
            let want = (cached.size - offset).min(len as u64);
            let mut reader = opened.store.blobs().reader(cached.hash);
            reader.seek(std::io::SeekFrom::Start(offset)).await?;
            reader.take(want).read_to_end(&mut data).await?;
        }

        Ok(data)
    }

    /// Writes `cached` out to `to`, unless a copy is there already.
    pub(crate) async fn export(&self, cached: Cached, to: &Path) -> Result<()> {
        if fs::metadata(to).is_ok_and(|meta| meta.len() == cached.size) {
            return Ok(());
        }

        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }

        let opened = self.open().await?;
        opened
            .store
            .blobs()
            .export(cached.hash, to)
            .finish()
            .await?;
        Ok(())
    }

    /// Conversation `session` closed: the files no other conversation
    /// opened go.
    pub(crate) async fn forget(&self, session: &SessionId) -> Result<()> {
        let opened = self.open().await?;
        let gone: Vec<String> = {
            let mut index = lock(&opened.index);
            let mut gone = Vec::new();

            for (hash, entry) in &mut index.files {
                if entry.sessions.remove(session) && entry.sessions.is_empty() {
                    gone.push(hash.clone());
                }
            }

            gone
        };

        self.drop_files(opened, &gone).await
    }

    /// Drops the files not opened for `KEEP`, then the oldest beyond
    /// `MAX_BYTES`.
    pub(crate) async fn sweep(&self) {
        match self.open().await {
            Ok(opened) => self.trim(opened, None).await,
            Err(e) => log::warn!("the chat file store did not open: {e:#}"),
        }
    }

    async fn trim(&self, opened: &Opened, keep: Option<Hash>) {
        let keep = keep.map(|hash| hash.to_hex());
        let gone = {
            let index = lock(&opened.index);
            stale(&index, now(), keep.as_deref())
        };

        if let Err(e) = self.drop_files(opened, &gone).await {
            log::warn!("dropping chat files failed: {e:#}");
        }
    }

    /// Records `cached` as opened now, by `session`.
    fn note(&self, opened: &Opened, cached: Cached, session: Option<&SessionId>) -> Result<()> {
        let mut index = lock(&opened.index);
        let entry = index
            .files
            .entry(cached.hash.to_hex())
            .or_insert_with(|| Entry {
                size: cached.size,
                opened: 0,
                sessions: BTreeSet::new(),
            });
        entry.opened = now();
        entry.sessions.extend(session.cloned());
        save(&self.dir, &index)
    }

    async fn drop_files(&self, opened: &Opened, gone: &[String]) -> Result<()> {
        if gone.is_empty() {
            return Ok(());
        }

        for hash in gone {
            opened.store.tags().delete(hash).await?;
        }

        {
            let mut index = lock(&opened.index);

            for hash in gone {
                index.files.remove(hash);
            }

            save(&self.dir, &index)?;
        }

        lock(&self.known).retain(|_, cached| !gone.contains(&cached.hash.to_hex()));
        remove_copies(&self.dir.join("open"), gone);
        Ok(())
    }

    async fn open(&self) -> Result<&Opened> {
        self.opened
            .get_or_try_init(|| async {
                fs::create_dir_all(&self.dir)?;
                let mut options = Options::new(&self.dir);
                options.gc = Some(GcConfig {
                    interval: GC_EVERY,
                    add_protected: None,
                });
                let store = FsStore::load_with_opts(self.dir.join("blobs.db"), options)
                    .await
                    .context("opening the chat file store")?;
                let index = fs::read(self.dir.join("index.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .unwrap_or_default();
                Ok(Opened {
                    store,
                    index: Mutex::new(index),
                })
            })
            .await
    }
}

/// Downloads `cached` from `machine` into `store`, resuming what it holds.
async fn download(
    fleet: &Fleet,
    store: &FsStore,
    machine: EndpointId,
    cached: Cached,
    progress: &mut impl FnMut(u64, u64),
) -> Result<()> {
    let connection = timeout(
        beep_mesh::OPEN_TIMEOUT,
        fleet.endpoint().connect(machine, iroh_blobs::ALPN),
    )
    .await
    .context("the machine did not answer in time")??;
    let fetching = store
        .remote()
        .fetch(connection.clone(), HashAndFormat::raw(cached.hash));
    let mut items = std::pin::pin!(fetching.stream());
    let result = loop {
        match items.next().await {
            Some(GetProgressItem::Progress(done)) => progress(done, cached.size),
            Some(GetProgressItem::Done(_)) => break Ok(()),
            Some(GetProgressItem::Error(e)) => break Err(e.into()),
            None => break Err(anyhow::anyhow!("the download ended early")),
        }
    };
    connection.close(0u32.into(), b"done");
    result
}

/// The files to drop: those not opened for `KEEP`, then the oldest until
/// the rest fit in `MAX_BYTES`. `keep` stays whatever its age.
fn stale(index: &Index, now: u64, keep: Option<&str>) -> Vec<String> {
    let mut files: Vec<(&String, &Entry)> = index
        .files
        .iter()
        .filter(|(hash, _)| Some(hash.as_str()) != keep)
        .collect();
    files.sort_by_key(|(_, entry)| std::cmp::Reverse(entry.opened));
    let mut held = keep
        .and_then(|hash| index.files.get(hash))
        .map_or(0, |entry| entry.size);
    let mut gone = Vec::new();

    for (hash, entry) in files {
        held = held.saturating_add(entry.size);

        if now.saturating_sub(entry.opened) > KEEP.as_secs() || held > MAX_BYTES {
            gone.push(hash.clone());
        }
    }

    gone
}

fn save(dir: &Path, index: &Index) -> Result<()> {
    let path = dir.join("index.json");
    let partial = dir.join("index.json.partial");
    fs::write(&partial, serde_json::to_vec(index)?)?;
    fs::rename(&partial, &path)?;
    Ok(())
}

/// Removes the opened copies of the files `gone` names.
fn remove_copies(dir: &Path, gone: &[String]) {
    let Ok(copies) = fs::read_dir(dir) else {
        return;
    };

    for copy in copies.flatten() {
        let name = copy.file_name();
        let name = name.to_string_lossy();
        let hash = name.split_once('-').map_or(name.as_ref(), |(hash, _)| hash);

        if gone.iter().any(|gone| gone == hash)
            && let Err(e) = fs::remove_file(copy.path())
        {
            log::debug!("could not remove an opened chat file: {e}");
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
#[path = "../tests/agents_file_cache.rs"]
mod tests;
