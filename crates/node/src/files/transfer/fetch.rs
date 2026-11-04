//! The target's half of a transfer: `fetch` checks sizes, limits and free
//! space, downloads into the store and moves the result into place.

use super::{
    Out, Ticker,
    place::{self, Folder, Journal, Partial, Reserved, Room, make_dirs_at, relative_path},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use beep_proto::node::transfer::{
    ContentKind, MANIFEST_NAME, MAX_BYTES, MAX_FILES, MAX_MANIFEST, Manifest, Stage,
};
use futures::StreamExt;
use iroh::{Endpoint, EndpointAddr, endpoint::Connection};
use iroh_blobs::{
    Hash, HashAndFormat,
    api::{Store, remote::GetProgressItem},
    format::collection::Collection,
    get::request::{get_hash_seq_and_sizes, get_verified_size},
    protocol::{ChunkRanges, GetRequest},
};
use std::{
    collections::HashSet,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    pin::pin,
    sync::Arc,
    time::Duration,
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    task::spawn_blocking,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

/// How long reaching the source may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How much is read from the store and written at a time.
const CHUNK: usize = 1 << 20;
/// The largest list of names a folder may have.
const MAX_NAMES: u64 = 64 << 20;
/// What every disk keeps free beyond the transfer.
const SPACE_MARGIN: u64 = 64 << 20;

/// The error a cancelled fetch ends with.
pub(super) const CANCELLED: &str = "the transfer was cancelled";

/// What to fetch and where it goes.
#[derive(Debug)]
pub(super) struct Job {
    pub(super) from: EndpointAddr,
    pub(super) content: HashAndFormat,
    pub(super) kind: ContentKind,
    /// A plain name, checked.
    pub(super) name: String,
    /// The target folder, open.
    pub(super) folder: Arc<Folder>,
    /// Whether a file lands executable.
    pub(super) executable: bool,
}

/// Where a fetch landed.
#[derive(Debug)]
pub(super) struct Fetched {
    pub(super) path: PathBuf,
    pub(super) size: u64,
    pub(super) files: u64,
}

/// One file or empty folder to write, by its path in the partial folder.
#[derive(Debug)]
struct Item {
    path: PathBuf,
    /// The file's blob; none for an empty folder.
    blob: Option<Hash>,
    executable: bool,
}

/// What the probe found: the files, their total size, and the most bytes
/// the download may receive.
#[derive(Debug)]
struct Plan {
    items: Vec<Item>,
    manifest: Option<Hash>,
    total: u64,
    expected: u64,
}

pub(super) struct Fetcher<'a> {
    pub(super) endpoint: &'a Endpoint,
    pub(super) store: &'a Store,
    /// Where the store keeps its data, for its free space.
    pub(super) store_dir: &'a Path,
    pub(super) journal: &'a Journal,
    /// The free space the running fetches hold.
    pub(super) room: &'a Arc<Room>,
    pub(super) cancel: &'a CancellationToken,
}

/// A fetch written out into its partial folder, holding its disk space
/// until it is placed.
struct Written {
    plan: Plan,
    partial: Partial,
    _room: Reserved,
}

/// Asks the source for every blob's verified size and the folder's names,
/// and checks limits and free space before any data moves. Writes under a
/// hidden partial folder, used by descriptor, and checks for a cancel
/// between chunks; only the final rename touches the final path.
pub(super) async fn fetch(cx: Fetcher<'_>, job: Job, tx: &mut Out<'_>) -> Result<Fetched> {
    // Kept from the collector while it downloads and is written out.
    let _tag = cx.store.tags().temp_tag(job.content).await?;
    let connection = connect(&cx, &job.from).await?;
    let result = transfer(&cx, &job, &connection, tx).await;
    connection.close(0u32.into(), b"done");
    let Written {
        plan,
        partial,
        _room,
    } = result?;
    let files = plan.items.iter().filter(|item| item.blob.is_some()).count() as u64;
    let size = plan.total;
    let Job { name, kind, .. } = job;
    spawn_blocking(move || match place::place(&partial, &name, kind) {
        Ok(path) => {
            partial.finish()?;
            Ok(Fetched { path, size, files })
        }
        Err(e) => {
            if let Err(left) = partial.remove() {
                log::warn!("a partial transfer stays until the next start: {left:#}");
            }
            Err(e)
        }
    })
    .await?
}

async fn connect(cx: &Fetcher<'_>, from: &EndpointAddr) -> Result<Connection> {
    let source = from.id;
    tokio::select! {
        () = cx.cancel.cancelled() => bail!(CANCELLED),
        connected = timeout(CONNECT_TIMEOUT, cx.endpoint.connect(from.clone(), iroh_blobs::ALPN)) => {
            connected
                .map_err(|_| anyhow!("could not reach the source machine ({source}) in time"))?
                .with_context(|| format!("could not reach the source machine ({source})"))
        }
    }
}

/// Probes, downloads and writes the content into a partial folder.
async fn transfer(
    cx: &Fetcher<'_>,
    job: &Job,
    connection: &Connection,
    tx: &mut Out<'_>,
) -> Result<Written> {
    let mut plan = cancellable(cx, probe(cx, job, connection)).await?;
    let room = reserve_space(cx, job, plan.total).await?;
    download(cx, job, connection, &plan, tx).await?;
    if let Some(manifest) = plan.manifest {
        apply_manifest(cx.store, manifest, job, &mut plan).await?;
    }
    let partial = {
        let (journal, folder) = (cx.journal.clone(), job.folder.clone());
        spawn_blocking(move || journal.start(folder)).await??
    };
    if let Err(e) = write_out(cx, job, &plan, &partial, tx).await {
        // Removed before the reply, so nothing is left when it goes out.
        let removed = spawn_blocking(move || partial.remove()).await?;
        if let Err(left) = removed {
            log::warn!("a partial transfer stays until the next start: {left:#}");
        }
        return Err(e);
    }
    Ok(Written {
        plan,
        partial,
        _room: room,
    })
}

async fn cancellable<T>(cx: &Fetcher<'_>, work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {
        () = cx.cancel.cancelled() => bail!(CANCELLED),
        result = work => result,
    }
}

/// Asks the source for the size of every blob, and the names of a
/// folder's files, before any data moves.
async fn probe(cx: &Fetcher<'_>, job: &Job, connection: &Connection) -> Result<Plan> {
    let hash = job.content.hash;
    let probe_failed = |e| anyhow!("asking the source machine what it sends failed: {e}");
    let too_large = || anyhow!("the transfer is larger than {} GiB", MAX_BYTES >> 30);
    if job.kind == ContentKind::File {
        let (size, _) = get_verified_size(connection, &hash)
            .await
            .map_err(probe_failed)?;
        ensure!(size <= MAX_BYTES, too_large());
        return Ok(Plan {
            items: vec![Item {
                path: PathBuf::from(&job.name),
                blob: Some(hash),
                executable: job.executable,
            }],
            manifest: None,
            total: size,
            expected: size,
        });
    }
    // The list, the names, an optional manifest and the files.
    let max_list = (MAX_FILES + 2) * 32;
    let (list, sizes) = get_hash_seq_and_sizes(connection, &hash, max_list, None)
        .await
        .map_err(probe_failed)?;
    let Some((&names_size, file_sizes)) = sizes.split_first() else {
        bail!("the folder's list is empty");
    };
    ensure!(names_size <= MAX_NAMES, "the folder's names are too large");
    let request = GetRequest::builder()
        .root(ChunkRanges::all())
        .child(0, ChunkRanges::all())
        .build(hash);
    cx.store
        .remote()
        .execute_get(connection.clone(), request)
        .await
        .map_err(|e| anyhow!("fetching the folder's names failed: {e}"))?;
    let collection = Collection::load(hash, cx.store)
        .await
        .context("reading the folder's names")?;
    ensure!(
        collection.len() == file_sizes.len(),
        "the folder's names do not match its files"
    );
    let mut plan = Plan {
        items: Vec::with_capacity(collection.len()),
        manifest: None,
        total: 0,
        expected: (list.len() as u64 * 32)
            .saturating_add(names_size)
            .saturating_add(file_sizes.iter().sum()),
    };
    for (at, ((name, hash), &size)) in collection.iter().zip(file_sizes).enumerate() {
        if at == 0 && name == MANIFEST_NAME {
            ensure!(size <= MAX_MANIFEST, "the folder's manifest is too large");
            plan.manifest = Some(*hash);
            continue;
        }
        plan.items.push(Item {
            path: Path::new(&job.name).join(relative_path(name)?),
            blob: Some(*hash),
            executable: false,
        });
        plan.total = plan.total.saturating_add(size);
    }
    ensure!(
        plan.items.len() as u64 <= MAX_FILES,
        "the folder holds more than {MAX_FILES} files"
    );
    ensure!(plan.total <= MAX_BYTES, too_large());
    Ok(plan)
}

/// Holds `need` bytes on the store's disk and the target folder's disk,
/// twice over when they are one disk, beside what the other running
/// fetches hold; fails when they do not have it.
async fn reserve_space(cx: &Fetcher<'_>, job: &Job, need: u64) -> Result<Reserved> {
    let (store_dir, folder, room) = (cx.store_dir.to_owned(), job.folder.clone(), cx.room.clone());
    spawn_blocking(move || {
        let store = place::space_at(&store_dir)?;
        room.reserve(need, store, folder.space()?, SPACE_MARGIN)
    })
    .await?
}

/// Downloads the content into the store, sending progress.
async fn download(
    cx: &Fetcher<'_>,
    job: &Job,
    connection: &Connection,
    plan: &Plan,
    tx: &mut Out<'_>,
) -> Result<()> {
    let progress = cx.store.remote().fetch(connection.clone(), job.content);
    let mut progress = pin!(progress.stream());
    let mut ticker = Ticker::new(Stage::Downloading, plan.total);
    loop {
        let item = tokio::select! {
            () = cx.cancel.cancelled() => bail!(CANCELLED),
            item = progress.next() => item,
        };
        match item {
            Some(GetProgressItem::Progress(done)) => {
                ensure!(
                    done <= plan.expected,
                    "the source sent more than the {} bytes it named",
                    plan.expected
                );
                ticker.tick(tx, done).await?;
            }
            Some(GetProgressItem::Done(_)) => break,
            Some(GetProgressItem::Error(e)) => {
                bail!("fetching from the source machine failed: {e}");
            }
            None => bail!("the download ended without a result"),
        }
    }
    ticker.finish(tx, plan.total).await
}

/// Marks the executable files and adds the empty folders the manifest
/// names.
async fn apply_manifest(store: &Store, hash: Hash, job: &Job, plan: &mut Plan) -> Result<()> {
    let bytes = store
        .blobs()
        .get_bytes(hash)
        .await
        .context("reading the folder's manifest")?;
    let manifest: Manifest =
        rmp_serde::from_slice(&bytes).context("reading the folder's manifest")?;
    let root = Path::new(&job.name);
    let executable = manifest
        .executable
        .iter()
        .map(|path| Ok(root.join(relative_path(path)?)))
        .collect::<Result<HashSet<_>>>()?;
    for item in &mut plan.items {
        item.executable = executable.contains(&item.path);
    }
    add_empty_folders(plan, root, &manifest.empty_folders)
}

/// Adds the empty folders a manifest names, which count against the file
/// limit like files.
fn add_empty_folders(plan: &mut Plan, root: &Path, folders: &[String]) -> Result<()> {
    ensure!(
        (plan.items.len() as u64).saturating_add(folders.len() as u64) <= MAX_FILES,
        "the folder holds more than {MAX_FILES} files and folders"
    );
    for folder in folders {
        plan.items.push(Item {
            path: root.join(relative_path(folder)?),
            blob: None,
            executable: false,
        });
    }
    Ok(())
}

/// Writes every item into the partial folder, sending progress.
async fn write_out(
    cx: &Fetcher<'_>,
    job: &Job,
    plan: &Plan,
    partial: &Partial,
    tx: &mut Out<'_>,
) -> Result<()> {
    let mut ticker = Ticker::new(Stage::Writing, plan.total);
    if job.kind == ContentKind::Folder {
        let dir = partial.dir().try_clone()?;
        let name = PathBuf::from(&job.name);
        spawn_blocking(move || make_dirs_at(&dir, &name))
            .await?
            .with_context(|| format!("making {}", job.name))?;
    }
    for item in &plan.items {
        let dir = partial.dir().try_clone()?;
        let (path, executable, blob) = (item.path.clone(), item.executable, item.blob);
        let opened = spawn_blocking(move || -> io::Result<Option<std::fs::File>> {
            if blob.is_none() {
                make_dirs_at(&dir, &path)?;
                return Ok(None);
            }
            let parent = make_dirs_at(&dir, path.parent().unwrap_or(Path::new("")))?;
            let name = path.file_name().ok_or(ErrorKind::InvalidInput)?;
            parent.create_file(name, executable).map(Some)
        })
        .await?
        .with_context(|| format!("creating {}", item.path.display()))?;
        let (Some(file), Some(blob)) = (opened, item.blob) else {
            continue;
        };
        let file = File::from_std(file);
        export(cx, &item.path, blob, file, &mut ticker, tx).await?;
    }
    ticker.finish(tx, ticker.done()).await
}

/// Copies one blob out of the store into `file`, counting on `ticker`.
async fn export(
    cx: &Fetcher<'_>,
    path: &Path,
    blob: Hash,
    mut file: File,
    ticker: &mut Ticker,
    tx: &mut Out<'_>,
) -> Result<()> {
    let mut reader = cx.store.blobs().reader(blob);
    let mut buffer = vec![0; CHUNK];
    loop {
        ensure!(!cx.cancel.is_cancelled(), CANCELLED);
        let read = reader
            .read(&mut buffer)
            .await
            .context("reading the store")?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .await
            .with_context(|| format!("writing {}", path.display()))?;
        ticker
            .tick(tx, ticker.done().saturating_add(read as u64))
            .await?;
    }
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(files: usize) -> Plan {
        Plan {
            items: (0..files)
                .map(|n| Item {
                    path: PathBuf::from(format!("out/{n}")),
                    blob: Some(Hash::new(n.to_string())),
                    executable: false,
                })
                .collect(),
            manifest: None,
            total: 0,
            expected: 0,
        }
    }

    #[test]
    fn empty_folders_count_against_the_file_limit() {
        let root = Path::new("out");
        let mut fits = plan(MAX_FILES as usize - 2);
        add_empty_folders(&mut fits, root, &["a".into(), "b/c".into()]).unwrap();
        assert_eq!(fits.items.len() as u64, MAX_FILES);
        assert_eq!(fits.items.last().unwrap().path, Path::new("out/b/c"));

        let mut over = plan(MAX_FILES as usize - 2);
        let folders: Vec<String> = (0..3).map(|n| format!("empty{n}")).collect();
        let refused = add_empty_folders(&mut over, root, &folders).unwrap_err();
        assert!(format!("{refused:#}").contains("more than"), "{refused:#}");
        assert_eq!(over.items.len() as u64, MAX_FILES - 2);
    }
}
