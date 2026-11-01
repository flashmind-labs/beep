//! The source's half: a file becomes one blob and a folder a collection of
//! its files by relative path. Files are hashed where they are
//! (`TryReference`), not copied into the store.

use anyhow::{Context, Result, ensure};
use beep_proto::node::{
    LinkTx,
    transfer::{Content, ContentKind, MAX_BYTES, MAX_FILES, Reply},
};
use iroh_blobs::{
    BlobFormat, HashAndFormat,
    api::{Store, TempTag, blobs::AddPathOptions, proto::ImportMode},
    format::collection::Collection,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

/// One file a folder holds.
#[derive(Debug)]
struct Entry {
    /// Its path inside the folder, parts joined with `/`.
    name: String,
    path: PathBuf,
    size: u64,
}

/// The files under `root`, sorted, with how many symlinks and special
/// files were left out.
#[derive(Debug, Default)]
struct Walked {
    entries: Vec<Entry>,
    skipped: u64,
}

fn walk(root: &Path) -> Result<Walked> {
    let mut walked = Walked::default();
    let mut total = 0u64;
    let mut folders = vec![(root.to_owned(), String::new())];
    while let Some((dir, prefix)) = folders.pop() {
        for item in fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
            let item = item?;
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                walked.skipped += 1;
                continue;
            };
            let name = format!("{prefix}{name}");
            let kind = item.file_type()?;
            if kind.is_dir() {
                folders.push((item.path(), format!("{name}/")));
            } else if kind.is_file() {
                let size = item.metadata()?.len();
                total = total.saturating_add(size);
                ensure!(
                    total <= MAX_BYTES,
                    "the folder holds more than {} GiB",
                    MAX_BYTES >> 30
                );
                walked.entries.push(Entry {
                    name,
                    path: item.path(),
                    size,
                });
                ensure!(
                    walked.entries.len() as u64 <= MAX_FILES,
                    "the folder holds more than {MAX_FILES} files"
                );
            } else {
                walked.skipped += 1;
            }
        }
    }
    walked.entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(walked)
}

/// What an offer made: the reply to send, and the tags that keep its blobs
/// in the store while it is open.
pub(super) struct Offered {
    pub(super) reply: Reply,
    pub(super) content: HashAndFormat,
    _tags: Vec<TempTag>,
}

async fn add_file(store: &Store, path: &Path) -> Result<TempTag> {
    store
        .blobs()
        .add_path_with_opts(AddPathOptions {
            path: path.to_owned(),
            format: BlobFormat::Raw,
            mode: ImportMode::TryReference,
        })
        .temp_tag()
        .await
        .with_context(|| format!("hashing {}", path.display()))
}

/// Imports `path`, already resolved inside the shared folders, sending
/// progress on `tx` per file of a folder. `from` is this node's address.
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
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    if metadata.is_file() {
        ensure!(
            metadata.len() <= MAX_BYTES,
            "the file is larger than {} GiB",
            MAX_BYTES >> 30
        );
        let tag = add_file(store, path).await?;
        let content = tag.hash_and_format();
        return Ok(Offered {
            reply: Reply::Offered {
                content: Content {
                    hash: *content.hash.as_bytes(),
                    kind: ContentKind::File,
                },
                from,
                name,
                size: metadata.len(),
                files: 1,
                skipped: 0,
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
    let mut tags = Vec::with_capacity(walked.entries.len() + 1);
    let mut collection = Collection::default();
    let mut done = 0;
    for entry in &walked.entries {
        let tag = add_file(store, &entry.path).await?;
        collection.push(entry.name.clone(), tag.hash());
        tags.push(tag);
        done += entry.size;
        tx.send(&Reply::Progress { done, total }).await?;
    }
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
        symlink("/etc/hostname", dir.join("link")).expect("linked");
        let walked = walk(&dir).unwrap();
        let names: Vec<_> = walked.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["frames/0001.png", "frames/deep/x.txt", "scene.blend"]
        );
        assert_eq!(walked.skipped, 1);
        assert_eq!(walked.entries[1].size, 2);
        fs::remove_dir_all(&dir).expect("removed");
    }
}
