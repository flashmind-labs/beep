//! Walking a project folder: the files sync keeps, the folders to watch,
//! and what the rules leave out. Hashing happens in the store, by
//! reference (`TryReference`), so a big file is read but not copied.

use super::{
    gitdir,
    rules::{Rules, Skip},
};
use crate::{
    files::fsat,
    files::transfer::offer::{Identity, check_same},
};
use anyhow::{Context, Result, ensure};
use beep_proto::node::sync::{Rule, SkippedPath};
use iroh_blobs::{
    BlobFormat,
    api::{Store, TempTag, blobs::AddPathOptions, proto::ImportMode},
};
use std::{
    fs::{self, Metadata},
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

/// The most files one project syncs, and their bytes. A bigger project
/// should be pinned to its machine.
pub(super) const MAX_FILES: usize = 50_000;
pub(super) const MAX_BYTES: u64 = 64 << 30;

/// What a file looked like on disk, to see whether it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Stat {
    pub(super) size: u64,
    pub(super) mtime_ns: i64,
    pub(super) ctime_ns: i64,
    pub(super) inode: u64,
    pub(super) device: u64,
    pub(super) executable: bool,
}

impl From<&Metadata> for Stat {
    fn from(meta: &Metadata) -> Self {
        let id = fsat::id_of_metadata(meta);
        Self {
            size: meta.len(),
            mtime_ns: fsat::mtime_ns(meta),
            ctime_ns: fsat::ctime_ns(meta),
            inode: id.inode,
            device: id.device,
            executable: fsat::is_executable(meta),
        }
    }
}

impl Stat {
    pub(super) fn mtime_ms(&self) -> i64 {
        self.mtime_ns.div_euclid(1_000_000)
    }
}

impl From<&Stat> for Identity {
    fn from(stat: &Stat) -> Self {
        Self {
            device: stat.device,
            inode: stat.inode,
        }
    }
}

/// A file sync keeps.
#[derive(Debug, Clone)]
pub(super) struct Found {
    /// Relative to the project folder, parts joined with `/`.
    pub(super) path: String,
    pub(super) abs: PathBuf,
    pub(super) stat: Stat,
}

/// What a walk found.
#[derive(Debug, Default)]
pub(super) struct Walked {
    pub(super) files: Vec<Found>,
    /// The folders sync keeps, the project folder first: what to watch.
    pub(super) dirs: Vec<PathBuf>,
    pub(super) skipped: Vec<SkippedPath>,
    /// More was left out than `skipped` holds.
    pub(super) skipped_more: bool,
}

impl Walked {
    pub(super) fn check_limits(&self) -> Result<()> {
        let bytes = self
            .files
            .iter()
            .fold(0u64, |sum, file| sum.saturating_add(file.stat.size));
        ensure!(
            self.files.len() <= MAX_FILES && bytes <= MAX_BYTES,
            "the project holds more than {MAX_FILES} files or {} GiB to sync; pin it \
             to keep it on one machine, or leave more out in .beepignore",
            MAX_BYTES >> 30
        );
        Ok(())
    }

    fn skip(
        &mut self,
        limit: usize,
        path: String,
        folder: bool,
        rule: Rule,
        marker: Option<String>,
    ) {
        if self.skipped.len() >= limit {
            self.skipped_more = true;
            return;
        }
        self.skipped.push(SkippedPath {
            path,
            folder,
            rule,
            marker,
        });
    }
}

/// Walks `root` under `rules`, listing at most `skipped` left-out paths.
pub(super) fn walk(root: &Path, rules: &Rules, skipped: usize) -> Result<Walked> {
    walk_inner(root, rules, skipped, false, false)
}

/// Git projects only scan metadata needed to seed a new checkout.
pub(super) fn walk_git(root: &Path, rules: &Rules, packed: bool) -> Result<Walked> {
    walk_inner(root, rules, 0, true, packed)
}

fn walk_inner(
    root: &Path,
    rules: &Rules,
    skipped: usize,
    git_only: bool,
    packed: bool,
) -> Result<Walked> {
    let mut walked = Walked {
        dirs: vec![root.to_owned()],
        ..Walked::default()
    };
    let mut bytes = 0u64;
    let mut stack = vec![(root.to_owned(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let listing = match fs::read_dir(&dir) {
            Ok(listing) => listing,
            // A folder that went away during the walk.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && dir != root => continue,
            Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
        };
        for item in listing {
            let item = item.with_context(|| format!("listing {}", dir.display()))?;
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                log::debug!("skipped a name that is not UTF-8 in {}", dir.display());
                continue;
            };
            let path = format!("{prefix}{name}");
            if git_only && path != gitdir::GIT && !gitdir::in_git(&path) {
                continue;
            }
            // A bootstrap pack supplies these objects instead of individual files.
            if packed && path == ".git/objects" {
                continue;
            }
            let Ok(kind) = item.file_type() else {
                continue;
            };
            let is_dir = kind.is_dir();
            match rules.skip(Path::new(&path), is_dir) {
                Some(Skip::Own) => continue,
                Some(Skip::Rule { rule, marker }) => {
                    walked.skip(skipped, path, is_dir, rule, marker);
                    continue;
                }
                None => {}
            }
            if is_dir {
                // A linked worktree or a submodule: its `.git` file names
                // this machine's paths, so it stays here whole.
                if path != gitdir::GIT
                    && (item.path().join(gitdir::GIT).is_file()
                        || rules.ignored_only() && item.path().join(gitdir::GIT).exists())
                {
                    walked.skip(skipped, path, true, Rule::Git, None);
                    continue;
                }
                if !gitdir::is_quiet(&path) {
                    walked.dirs.push(item.path());
                }
                stack.push((item.path(), format!("{path}/")));
            } else if kind.is_file() {
                // Refs are asked of git, packed or not.
                if gitdir::is_ref(&path) {
                    continue;
                }
                let meta = match item.metadata() {
                    Ok(meta) => meta,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e).with_context(|| format!("reading {path}")),
                };
                bytes = bytes.saturating_add(meta.len());
                walked.files.push(Found {
                    path,
                    abs: item.path(),
                    stat: Stat::from(&meta),
                });
                ensure!(
                    walked.files.len() <= MAX_FILES && bytes <= MAX_BYTES,
                    "the project holds more than {MAX_FILES} files or {} GiB to sync; pin it \
                     to keep it on one machine, or leave more out in .beepignore",
                    MAX_BYTES >> 30
                );
            } else {
                walked.skip(skipped, path, false, Rule::Link, None);
            }
        }
    }
    walked.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(walked)
}

/// Hashes `found` into the store where it lies, refusing it when the path
/// no longer names the file the walk saw.
pub(super) async fn import(store: &Store, found: &Found) -> Result<TempTag> {
    import_as(store, found, ImportMode::TryReference).await
}

/// Generated bootstrap packs can be replaced once the store owns their bytes.
pub(super) async fn import_copy(store: &Store, found: &Found) -> Result<TempTag> {
    import_as(store, found, ImportMode::Copy).await
}

async fn import_as(store: &Store, found: &Found, mode: ImportMode) -> Result<TempTag> {
    let identity = Identity::from(&found.stat);
    let check =
        |path: PathBuf| async move { spawn_blocking(move || check_same(&path, identity)).await? };
    check(found.abs.clone()).await?;
    let tag = store
        .blobs()
        .add_path_with_opts(AddPathOptions {
            path: found.abs.clone(),
            format: BlobFormat::Raw,
            mode,
        })
        .temp_tag()
        .await
        .with_context(|| format!("hashing {}", found.path))?;
    check(found.abs.clone()).await?;
    Ok(tag)
}

#[cfg(all(test, unix))]
#[path = "../../tests/files_sync_scan.rs"]
mod tests;
