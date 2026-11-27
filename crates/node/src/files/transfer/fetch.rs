//! The target's half of a transfer: `fetch` checks sizes, limits and free
//! space, downloads into the store and moves the result into place.

use super::progress::{Out, Ticker};
use crate::files::place::{self, Folder, Partial, Reserved, make_dirs_at, relative_path};
use crate::files::sync::Parts;
use anyhow::{Context, Result, anyhow, bail, ensure};
use beep_proto::node::transfer::{
    Content, ContentKind, MANIFEST_NAME, MAX_BYTES, MAX_FILES, MAX_MANIFEST, Manifest, Stage,
};
use futures::StreamExt;
use iroh::{EndpointAddr, endpoint::Connection};
use iroh_blobs::{
    BlobFormat, Hash, HashAndFormat,
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
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    task::spawn_blocking,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

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
    /// A plain name, checked.
    pub(super) name: String,
    /// The target folder, open.
    pub(super) folder: Arc<Folder>,
    /// Whether a file lands executable.
    pub(super) executable: bool,
}

impl Job {
    pub(super) fn new(
        from: EndpointAddr,
        content: &Content,
        name: String,
        folder: Arc<Folder>,
        executable: bool,
    ) -> Self {
        let hash = Hash::from_bytes(content.hash);

        Self {
            from,
            content: match content.kind {
                ContentKind::File => HashAndFormat::raw(hash),
                ContentKind::Folder => HashAndFormat::hash_seq(hash),
            },
            name,
            folder,
            executable,
        }
    }
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
pub(super) async fn fetch(
    parts: &Parts,
    cancel: &CancellationToken,
    job: Job,
    tx: &mut Out<'_>,
) -> Result<Fetched> {
    // Kept from the collector while it downloads and is written out.
    let _tag = parts.store.tags().temp_tag(job.content).await?;
    let connection = connect(parts, cancel, &job.from).await?;
    let result = transfer(parts, cancel, &job, &connection, tx).await;
    connection.close(0u32.into(), b"done");
    let Written {
        plan,
        partial,
        _room,
    } = result?;
    let files = plan.items.iter().filter(|item| item.blob.is_some()).count() as u64;
    let size = plan.total;
    let kind = match job.content.format {
        BlobFormat::Raw => ContentKind::File,
        BlobFormat::HashSeq => ContentKind::Folder,
    };
    let name = job.name;
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

async fn connect(
    parts: &Parts,
    cancel: &CancellationToken,
    from: &EndpointAddr,
) -> Result<Connection> {
    let source = from.id;
    tokio::select! {
        () = cancel.cancelled() => bail!(CANCELLED),
        connected = timeout(beep_mesh::OPEN_TIMEOUT, parts.endpoint.connect(from.clone(), iroh_blobs::ALPN)) => {
            connected
                .map_err(|_| anyhow!("could not reach the source machine ({source}) in time"))?
                .with_context(|| format!("could not reach the source machine ({source})"))
        }
    }
}

/// Probes, downloads and writes the content into a partial folder.
async fn transfer(
    parts: &Parts,
    cancel: &CancellationToken,
    job: &Job,
    connection: &Connection,
    tx: &mut Out<'_>,
) -> Result<Written> {
    let mut plan = cancellable(cancel, probe(&parts.store, job, connection)).await?;
    let room = reserve_space(parts, job, plan.total).await?;
    download(&parts.store, cancel, job, connection, &plan, tx).await?;

    if let Some(manifest) = plan.manifest {
        apply_manifest(&parts.store, manifest, job, &mut plan).await?;
    }

    let partial = {
        let (journal, folder) = (parts.journal.clone(), job.folder.clone());
        spawn_blocking(move || journal.start(folder)).await??
    };

    if let Err(e) = write_out(&parts.store, cancel, job, &plan, &partial, tx).await {
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

async fn cancellable<T>(
    cancel: &CancellationToken,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        () = cancel.cancelled() => bail!(CANCELLED),
        result = work => result,
    }
}

/// Asks the source for the size of every blob, and the names of a
/// folder's files, before any data moves.
async fn probe(store: &Store, job: &Job, connection: &Connection) -> Result<Plan> {
    let hash = job.content.hash;
    let probe_failed = |e| anyhow!("asking the source machine what it sends failed: {e}");
    let too_large = || anyhow!("the transfer is larger than {} GiB", MAX_BYTES >> 30);

    if job.content.format == BlobFormat::Raw {
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
    store
        .remote()
        .execute_get(connection.clone(), request)
        .await
        .map_err(|e| anyhow!("fetching the folder's names failed: {e}"))?;
    let collection = Collection::load(hash, store)
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
async fn reserve_space(parts: &Parts, job: &Job, need: u64) -> Result<Reserved> {
    let (store_dir, folder, room) = (
        parts.store_dir.clone(),
        job.folder.clone(),
        parts.room.clone(),
    );
    spawn_blocking(move || {
        let store = place::space_at(&store_dir)?;
        room.reserve(need, store, folder.space()?, SPACE_MARGIN)
    })
    .await?
}

/// Downloads the content into the store, sending progress.
async fn download(
    store: &Store,
    cancel: &CancellationToken,
    job: &Job,
    connection: &Connection,
    plan: &Plan,
    tx: &mut Out<'_>,
) -> Result<()> {
    let progress = store.remote().fetch(connection.clone(), job.content);
    let mut progress = pin!(progress.stream());
    let mut ticker = Ticker::new(Stage::Downloading, plan.total);

    loop {
        let item = tokio::select! {
            () = cancel.cancelled() => bail!(CANCELLED),
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
        beep_proto::lenient::from_slice(&bytes).context("reading the folder's manifest")?;
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
    store: &Store,
    cancel: &CancellationToken,
    job: &Job,
    plan: &Plan,
    partial: &Partial,
    tx: &mut Out<'_>,
) -> Result<()> {
    let mut ticker = Ticker::new(Stage::Writing, plan.total);

    if job.content.format == BlobFormat::HashSeq {
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
        export(store, cancel, &item.path, blob, file, &mut ticker, tx).await?;
    }

    ticker.finish(tx, ticker.done()).await
}

/// Copies one blob out of the store into `file`, counting on `ticker`.
async fn export(
    store: &Store,
    cancel: &CancellationToken,
    path: &Path,
    blob: Hash,
    mut file: File,
    ticker: &mut Ticker,
    tx: &mut Out<'_>,
) -> Result<()> {
    let mut reader = store.blobs().reader(blob);
    let mut buffer = vec![0; CHUNK];

    loop {
        ensure!(!cancel.is_cancelled(), CANCELLED);
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
#[path = "../../tests/files_transfer_fetch.rs"]
mod tests;
