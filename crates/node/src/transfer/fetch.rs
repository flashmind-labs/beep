//! The target's half: download from the source with verified streaming
//! into the store, write it out under a hidden partial folder, then rename
//! it into place. A cancel is checked between every chunk, and only the
//! last step puts anything at the final path.

use super::place::{self, Journal, relative_path};
use anyhow::{Context, Result, anyhow, bail, ensure};
use beep_proto::node::{
    LinkTx,
    transfer::{ContentKind, MAX_BYTES, MAX_FILES, Reply},
};
use futures::StreamExt;
use iroh::{Endpoint, EndpointAddr};
use iroh_blobs::{
    Hash, HashAndFormat,
    api::{Store, remote::GetProgressItem},
    format::collection::Collection,
};
use std::{
    path::{Path, PathBuf},
    pin::pin,
    time::Duration,
};
use tokio::{
    fs::{self, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
    task::spawn_blocking,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

/// How long reaching the source may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How much is read from the store and written at a time.
const CHUNK: usize = 1 << 20;
/// Progress goes out at least this many bytes apart, or each percent.
const PROGRESS_STEP: u64 = 1 << 20;

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
    pub(super) size: u64,
    /// The target folder, resolved and checked.
    pub(super) folder: PathBuf,
}

/// Where a fetch landed.
#[derive(Debug)]
pub(super) struct Fetched {
    pub(super) path: PathBuf,
    pub(super) size: u64,
    pub(super) files: u64,
}

/// The blobs this node needs, by path inside the partial folder.
struct Item {
    path: PathBuf,
    hash: Hash,
}

pub(super) struct Fetcher<'a> {
    pub(super) endpoint: &'a Endpoint,
    pub(super) store: &'a Store,
    pub(super) journal: &'a Journal,
    pub(super) cancel: &'a CancellationToken,
}

pub(super) async fn fetch(cx: Fetcher<'_>, job: Job, tx: &mut LinkTx) -> Result<Fetched> {
    ensure!(
        job.size <= MAX_BYTES,
        "the transfer is larger than {} GiB",
        MAX_BYTES >> 30
    );
    // Kept from the collector while it downloads and is written out.
    let _tag = cx.store.tags().temp_tag(job.content).await?;
    download(&cx, &job, tx).await?;
    let items = items(cx.store, &job).await?;
    let partial = {
        let (journal, folder, name) = (cx.journal.clone(), job.folder.clone(), job.name.clone());
        spawn_blocking(move || journal.start(&folder, &name)).await??
    };
    let item = partial.item(&job.name);
    let written = write_out(cx.store, &item, &items, cx.cancel).await;
    let Job {
        folder, name, kind, ..
    } = job;
    let files = items.len() as u64;
    spawn_blocking(move || {
        let placed = written.and_then(|size| {
            if kind == ContentKind::Folder && items.is_empty() {
                std::fs::create_dir(&item)?;
            }
            Ok((size, place::place(&item, &folder, &name, kind)?))
        });
        match placed {
            Ok((size, path)) => {
                partial.finish()?;
                Ok(Fetched { path, size, files })
            }
            Err(e) => {
                if let Err(left) = partial.remove() {
                    log::warn!("a partial transfer stays until the next start: {left:#}");
                }
                Err(e)
            }
        }
    })
    .await?
}

/// Downloads the content into the store, sending progress.
async fn download(cx: &Fetcher<'_>, job: &Job, tx: &mut LinkTx) -> Result<()> {
    let source = job.from.id;
    let connection = tokio::select! {
        () = cx.cancel.cancelled() => bail!(CANCELLED),
        connected = timeout(CONNECT_TIMEOUT, cx.endpoint.connect(job.from.clone(), iroh_blobs::ALPN)) => {
            connected
                .map_err(|_| anyhow!("could not reach the source machine ({source}) in time"))?
                .with_context(|| format!("could not reach the source machine ({source})"))?
        }
    };
    let progress = cx.store.remote().fetch(connection.clone(), job.content);
    let mut progress = pin!(progress.stream());
    let mut sent = 0;
    let step = (job.size / 100).max(PROGRESS_STEP);
    let result = loop {
        let item = tokio::select! {
            () = cx.cancel.cancelled() => break Err(anyhow!(CANCELLED)),
            item = progress.next() => item,
        };
        match item {
            Some(GetProgressItem::Progress(done)) => {
                if done >= sent + step {
                    sent = done;
                    tx.send(&Reply::Progress {
                        done,
                        total: job.size,
                    })
                    .await?;
                }
            }
            Some(GetProgressItem::Done(_)) => break Ok(()),
            Some(GetProgressItem::Error(e)) => {
                break Err(anyhow!("fetching from the source machine failed: {e}"));
            }
            None => break Err(anyhow!("the download ended without a result")),
        }
    };
    connection.close(0u32.into(), b"done");
    result?;
    tx.send(&Reply::Progress {
        done: job.size,
        total: job.size,
    })
    .await
}

/// The blobs to write, by their path under the item.
async fn items(store: &Store, job: &Job) -> Result<Vec<Item>> {
    match job.kind {
        ContentKind::File => Ok(vec![Item {
            path: PathBuf::new(),
            hash: job.content.hash,
        }]),
        ContentKind::Folder => {
            let collection = Collection::load(job.content.hash, store)
                .await
                .context("reading the folder's list")?;
            ensure!(
                collection.len() as u64 <= MAX_FILES,
                "the folder holds more than {MAX_FILES} files"
            );
            collection
                .iter()
                .map(|(name, hash)| {
                    Ok(Item {
                        path: relative_path(name)?,
                        hash: *hash,
                    })
                })
                .collect()
        }
    }
}

/// Writes every item under `item`, the path in the partial folder. Gives
/// the bytes written.
async fn write_out(
    store: &Store,
    item: &Path,
    items: &[Item],
    cancel: &CancellationToken,
) -> Result<u64> {
    let mut size = 0;
    for entry in items {
        // A file's one entry has no path of its own.
        let target = if entry.path.as_os_str().is_empty() {
            item.to_owned()
        } else {
            item.join(&entry.path)
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("making {}", parent.display()))?;
        }
        size += export(store, entry.hash, &target, cancel).await?;
    }
    Ok(size)
}

/// Copies one blob out of the store into a new file at `target`.
async fn export(
    store: &Store,
    hash: Hash,
    target: &Path,
    cancel: &CancellationToken,
) -> Result<u64> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .await
        .with_context(|| format!("creating {}", target.display()))?;
    let mut reader = store.blobs().reader(hash);
    let mut buffer = vec![0; CHUNK];
    let mut written = 0;
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
            .with_context(|| format!("writing {}", target.display()))?;
        written += read as u64;
    }
    file.flush().await?;
    Ok(written)
}
