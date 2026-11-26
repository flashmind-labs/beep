//! Walking a project folder: the files sync keeps, the folders to watch,
//! and what the rules leave out. Hashing happens in the store, by
//! reference (`TryReference`), so a big file is read but not copied.

use super::{
    disk::{Found, Stat},
    gitdir,
    rules::{Rules, Skip},
};
use crate::fs::fsat;
use anyhow::{Context, Result, ensure};
use beep_proto::node::sync::{Rule, SkippedPath};
use iroh_blobs::{
    BlobFormat,
    api::{Store, TempTag, blobs::AddPathOptions, proto::ImportMode},
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};
use tokio::task::spawn_blocking;

/// The most files one project syncs, and their bytes. A bigger project
/// should be pinned to its machine.
pub(super) const MAX_FILES: usize = 50_000;
pub(super) const MAX_BYTES: u64 = 64 << 30;

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

fn within_limits(files: usize, bytes: u64) -> Result<()> {
    ensure!(
        files <= MAX_FILES && bytes <= MAX_BYTES,
        "the project holds more than {MAX_FILES} files or {} GiB to sync; pin it \
         to keep it on one machine, or leave more out in .beepignore",
        MAX_BYTES >> 30
    );
    Ok(())
}

impl Walked {
    pub(super) fn check_limits(&self) -> Result<()> {
        let bytes = self
            .files
            .iter()
            .fold(0u64, |sum, file| sum.saturating_add(file.stat.size));
        within_limits(self.files.len(), bytes)
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
    walk_inner(root, rules, skipped, false, false, None)
}

/// Git projects only scan metadata needed to seed a new checkout.
pub(super) fn walk_git(root: &Path, rules: &Rules, packed: bool) -> Result<Walked> {
    walk_inner(root, rules, 0, true, packed, None)
}

/// Inspects only event paths, walking a subtree only when its directory changed.
pub(super) fn walk_changed(
    root: &Path,
    rules: &Rules,
    paths: &BTreeSet<PathBuf>,
    git_only: bool,
    packed: bool,
) -> Result<Walked> {
    walk_inner(root, rules, 0, git_only, packed, Some(paths))
}

fn walk_inner(
    root: &Path,
    rules: &Rules,
    skipped: usize,
    git_only: bool,
    packed: bool,
    selected: Option<&BTreeSet<PathBuf>>,
) -> Result<Walked> {
    let mut walked = Walked {
        dirs: if selected.is_none() {
            vec![root.to_owned()]
        } else {
            Vec::new()
        },
        ..Walked::default()
    };
    let mut bytes = 0u64;
    let mut stack = Vec::new();

    if let Some(selected) = selected {
        for relative in selected {
            ensure!(
                relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
                "invalid changed path"
            );
            let Some(text) = relative.to_str() else {
                continue;
            };
            #[cfg(windows)]
            let path = text.replace('\\', "/");
            #[cfg(not(windows))]
            let path = text.to_owned();
            if git_only && !gitdir::in_git(&path) && path != gitdir::GIT
                || packed && path.starts_with(".git/objects")
            {
                continue;
            }

            // An event for a child of a directory replaced by a symlink must stay local.
            let mut absolute = root.to_owned();
            let mut metadata = None;
            for part in relative.components() {
                absolute.push(part);
                metadata = match fs::symlink_metadata(&absolute) {
                    Ok(meta) if !meta.is_symlink() => Some(meta),
                    Ok(_) => None,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::NotFound
                            || e.kind() == std::io::ErrorKind::NotADirectory =>
                    {
                        None
                    }
                    Err(e) => return Err(e).with_context(|| format!("reading {path}")),
                };
                if metadata.is_none() {
                    break;
                }
            }
            let Some(meta) = metadata else {
                continue;
            };
            if rules.skips_any(relative, meta.is_dir()) || gitdir::is_ref(&path) {
                continue;
            }
            if meta.is_dir() {
                if path != gitdir::GIT
                    && (absolute.join(gitdir::GIT).is_file()
                        || rules.ignored_only() && absolute.join(gitdir::GIT).exists())
                {
                    continue;
                }
                walked.dirs.push(absolute.clone());
                stack.push((absolute, format!("{path}/")));
            } else if meta.is_file() {
                bytes = bytes.saturating_add(meta.len());
                walked.files.push(Found {
                    path,
                    abs: absolute,
                    stat: Stat::from(&meta),
                });
            }
        }
    } else {
        stack.push((root.to_owned(), String::new()));
    }

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
                within_limits(walked.files.len(), bytes)?;
            } else {
                walked.skip(skipped, path, false, Rule::Link, None);
            }
        }
    }

    walked.files.sort_by(|a, b| a.path.cmp(&b.path));
    walked.files.dedup_by(|a, b| a.path == b.path);
    walked.check_limits()?;
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
    let identity = found.stat.id();
    let check =
        |path: PathBuf| async move { spawn_blocking(move || fsat::check_same(&path, identity)).await? };
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
