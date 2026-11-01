//! The source's half: a file becomes one blob and a folder a collection of
//! its files by relative path, led by a manifest when it has executable
//! files or empty folders. Files are hashed where they are
//! (`TryReference`), not copied into the store. A file swapped for a link
//! or another file after the walk is refused rather than sent.

use super::Ticker;
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::{
    LinkTx,
    transfer::{Content, ContentKind, MANIFEST_NAME, MAX_BYTES, MAX_FILES, Manifest, Reply, Stage},
};
use futures::StreamExt;
use iroh_blobs::{
    BlobFormat, HashAndFormat,
    api::{
        Store, TempTag,
        blobs::{AddPathOptions, AddProgressItem},
        proto::ImportMode,
    },
    format::collection::Collection,
};
use std::{
    fs::{self, Metadata, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

/// Which file on which disk, to see that a path still names the file the
/// walk found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn of(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

/// One file a folder holds.
#[derive(Debug)]
struct Entry {
    /// Its path inside the folder, parts joined with `/`.
    name: String,
    path: PathBuf,
    size: u64,
    executable: bool,
    identity: Identity,
}

/// The files under `root`, sorted, the folders that hold nothing, and how
/// many symlinks and special files were left out.
#[derive(Debug, Default)]
struct Walked {
    entries: Vec<Entry>,
    empty_folders: Vec<String>,
    skipped: u64,
}

fn is_executable(metadata: &Metadata) -> bool {
    metadata.permissions().mode() & 0o111 != 0
}

fn walk(root: &Path) -> Result<Walked> {
    let mut walked = Walked::default();
    let mut total = 0u64;
    let mut folders = vec![(root.to_owned(), String::new())];
    while let Some((dir, prefix)) = folders.pop() {
        let mut kept = 0;
        for item in fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
            let item = item?;
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                walked.skipped += 1;
                continue;
            };
            let name = format!("{prefix}{name}");
            let kind = item.file_type()?;
            if kind.is_dir() {
                kept += 1;
                folders.push((item.path(), format!("{name}/")));
            } else if kind.is_file() {
                kept += 1;
                let metadata = item.metadata()?;
                total = total.saturating_add(metadata.len());
                ensure!(
                    total <= MAX_BYTES,
                    "the folder holds more than {} GiB",
                    MAX_BYTES >> 30
                );
                walked.entries.push(Entry {
                    name,
                    path: item.path(),
                    size: metadata.len(),
                    executable: is_executable(&metadata),
                    identity: Identity::of(&metadata),
                });
                ensure!(
                    walked.entries.len() as u64 <= MAX_FILES,
                    "the folder holds more than {MAX_FILES} files"
                );
            } else {
                walked.skipped += 1;
            }
        }
        if kept == 0 && !prefix.is_empty() {
            walked
                .empty_folders
                .push(prefix.trim_end_matches('/').to_owned());
        }
    }
    walked.entries.sort_by(|a, b| a.name.cmp(&b.name));
    walked.empty_folders.sort();
    Ok(walked)
}

/// What an offer made: the reply to send, and the tags that keep its blobs
/// in the store while it is open.
pub(super) struct Offered {
    pub(super) reply: Reply,
    pub(super) content: HashAndFormat,
    _tags: Vec<TempTag>,
}

/// Fails unless `path` opens, without following a link, as the file
/// `identity` names.
fn check_same(path: &Path, identity: Identity) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let opened = Identity::of(&file.metadata()?);
    let named = Identity::of(&fs::symlink_metadata(path)?);
    if opened != identity || named != identity {
        bail!("{} changed while it was being sent", path.display());
    }
    Ok(())
}

async fn checked(path: &Path, identity: Identity) -> Result<()> {
    let path = path.to_owned();
    spawn_blocking(move || check_same(&path, identity)).await?
}

/// Hashes one file into the store, reporting the bytes hashed on top of
/// `before`.
async fn add_file(
    store: &Store,
    entry: &Entry,
    before: u64,
    ticker: &mut Ticker,
    tx: &mut LinkTx,
) -> Result<TempTag> {
    checked(&entry.path, entry.identity).await?;
    let progress = store.blobs().add_path_with_opts(AddPathOptions {
        path: entry.path.clone(),
        format: BlobFormat::Raw,
        mode: ImportMode::TryReference,
    });
    let mut progress = std::pin::pin!(progress.stream().await);
    let tag = loop {
        match progress.next().await {
            Some(AddProgressItem::OutboardProgress(done) | AddProgressItem::CopyProgress(done)) => {
                ticker.tick(tx, before.saturating_add(done)).await?;
            }
            Some(AddProgressItem::Done(tag)) => break tag,
            Some(AddProgressItem::Error(e)) => {
                return Err(e).with_context(|| format!("hashing {}", entry.path.display()));
            }
            Some(AddProgressItem::Size(_) | AddProgressItem::CopyDone) => {}
            None => bail!("hashing {} ended without a result", entry.path.display()),
        }
    };
    // The import read the file by path: it must still be the one walked.
    checked(&entry.path, entry.identity).await?;
    Ok(tag)
}

/// Imports `path`, already resolved inside the shared folders, sending
/// hashing progress on `tx`. `from` is this node's address.
pub(super) async fn offer(
    store: &Store,
    path: &Path,
    from: iroh::EndpointAddr,
    tx: &mut LinkTx,
) -> Result<Offered> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("{} has no name to send it under", path.display()))?
        .to_owned();
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    if metadata.is_file() {
        ensure!(
            metadata.len() <= MAX_BYTES,
            "the file is larger than {} GiB",
            MAX_BYTES >> 30
        );
        let entry = Entry {
            name: name.clone(),
            path: path.to_owned(),
            size: metadata.len(),
            executable: is_executable(&metadata),
            identity: Identity::of(&metadata),
        };
        let mut ticker = Ticker::new(Stage::Hashing, entry.size);
        let tag = add_file(store, &entry, 0, &mut ticker, tx).await?;
        ticker.finish(tx, entry.size).await?;
        let content = tag.hash_and_format();
        return Ok(Offered {
            reply: Reply::Offered {
                content: Content {
                    hash: *content.hash.as_bytes(),
                    kind: ContentKind::File,
                },
                from,
                name,
                size: entry.size,
                files: 1,
                skipped: 0,
                executable: entry.executable,
            },
            content,
            _tags: vec![tag],
        });
    }
    ensure!(
        metadata.is_dir(),
        "{} is neither a file nor a folder",
        path.display()
    );
    let root = path.to_owned();
    let walked = spawn_blocking(move || walk(&root)).await??;
    let total: u64 = walked.entries.iter().map(|entry| entry.size).sum();
    let mut tags = Vec::with_capacity(walked.entries.len() + 2);
    let mut collection = Collection::default();
    let manifest = Manifest {
        executable: walked
            .entries
            .iter()
            .filter(|entry| entry.executable)
            .map(|entry| entry.name.clone())
            .collect(),
        empty_folders: walked.empty_folders.clone(),
    };
    if manifest != Manifest::default() {
        let bytes = rmp_serde::to_vec_named(&manifest).context("encoding the manifest")?;
        let tag = store
            .blobs()
            .add_bytes(bytes)
            .temp_tag()
            .await
            .context("storing the manifest")?;
        collection.push(MANIFEST_NAME.to_owned(), tag.hash());
        tags.push(tag);
    }
    let mut ticker = Ticker::new(Stage::Hashing, total);
    let mut done = 0u64;
    for entry in &walked.entries {
        let tag = add_file(store, entry, done, &mut ticker, tx).await?;
        collection.push(entry.name.clone(), tag.hash());
        tags.push(tag);
        done = done.saturating_add(entry.size);
        ticker.tick(tx, done).await?;
    }
    ticker.finish(tx, done).await?;
    let root = collection
        .store(store)
        .await
        .context("storing the folder's list")?;
    let content = root.hash_and_format();
    tags.push(root);
    Ok(Offered {
        reply: Reply::Offered {
            content: Content {
                hash: *content.hash.as_bytes(),
                kind: ContentKind::Folder,
            },
            from,
            name,
            size: total,
            files: walked.entries.len() as u64,
            skipped: walked.skipped,
            executable: false,
        },
        content,
        _tags: tags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn a_folder_walk_keeps_files_and_skips_links() {
        let dir = std::env::temp_dir().join(format!("beep-node-walk-{}", std::process::id()));
        // A leftover from an earlier run with the same process id.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("frames/deep")).expect("made");
        fs::create_dir_all(dir.join("empty")).expect("made");
        fs::write(dir.join("scene.blend"), "abc").expect("written");
        fs::write(dir.join("frames/0001.png"), "1").expect("written");
        fs::write(dir.join("frames/deep/x.txt"), "xy").expect("written");
        fs::create_dir_all(dir.join("links-only")).expect("made");
        symlink("/etc/hostname", dir.join("link")).expect("linked");
        symlink("/etc/hostname", dir.join("links-only/link")).expect("linked");
        fs::set_permissions(dir.join("scene.blend"), fs::Permissions::from_mode(0o755))
            .expect("made executable");
        let walked = walk(&dir).unwrap();
        let names: Vec<_> = walked.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["frames/0001.png", "frames/deep/x.txt", "scene.blend"]
        );
        assert_eq!(walked.skipped, 2);
        assert_eq!(walked.entries[1].size, 2);
        assert!(walked.entries[2].executable);
        assert!(!walked.entries[0].executable);
        assert_eq!(walked.empty_folders, ["empty", "links-only"]);
        fs::remove_dir_all(&dir).expect("removed");
    }

    #[test]
    fn a_file_swapped_after_the_walk_is_refused() {
        let dir = std::env::temp_dir().join(format!("beep-node-swap-{}", std::process::id()));
        // A leftover from an earlier run with the same process id.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("made");
        let file = dir.join("render.png");
        fs::write(&file, "a").expect("written");
        fs::write(dir.join("other.png"), "b").expect("written");
        let walked = Identity::of(&fs::metadata(&file).expect("exists"));
        check_same(&file, walked).unwrap();

        fs::remove_file(&file).expect("removed");
        symlink(dir.join("other.png"), &file).expect("linked");
        assert!(check_same(&file, walked).is_err());

        fs::remove_file(&file).expect("removed");
        fs::rename(dir.join("other.png"), &file).expect("moved");
        let changed = check_same(&file, walked).unwrap_err();
        assert!(format!("{changed:#}").contains("changed while it was being sent"));
        fs::remove_dir_all(&dir).expect("removed");
    }
}
