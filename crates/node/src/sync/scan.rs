//! Walking a project folder: the files sync keeps, the folders to watch,
//! and what the rules leave out. Hashing happens in the store, by
//! reference (`TryReference`), so a big file is read but not copied.

use super::rules::{GitIgnored, Rules, Skip};
use crate::transfer::offer::{Identity, check_same};
use anyhow::{Context, Result, ensure};
use beep_proto::node::sync::{Rule, SkippedPath};
use iroh_blobs::{
    BlobFormat,
    api::{Store, TempTag, blobs::AddPathOptions, proto::ImportMode},
};
use std::{
    fs::{self, Metadata},
    os::unix::fs::{MetadataExt, PermissionsExt},
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

impl Stat {
    pub(super) fn of(meta: &Metadata) -> Self {
        Self {
            size: meta.len(),
            mtime_ns: meta.mtime().saturating_mul(1_000_000_000) + meta.mtime_nsec(),
            ctime_ns: meta.ctime().saturating_mul(1_000_000_000) + meta.ctime_nsec(),
            inode: meta.ino(),
            device: meta.dev(),
            executable: meta.permissions().mode() & 0o111 != 0,
        }
    }

    pub(super) fn mtime_ms(&self) -> i64 {
        self.mtime_ns.div_euclid(1_000_000)
    }

    fn identity(&self) -> Identity {
        Identity {
            device: self.device,
            inode: self.inode,
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
    /// Files of a git repository that git carries.
    pub(super) through_git: u64,
}

impl Walked {
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
    let git = if rules.is_git() {
        Some(GitIgnored::read(root)?)
    } else {
        None
    };
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
                walked.dirs.push(item.path());
                stack.push((item.path(), format!("{path}/")));
            } else if kind.is_file() {
                if let Some(git) = &git
                    && !git.contains(&path)
                {
                    walked.through_git += 1;
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
                    stat: Stat::of(&meta),
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
    let identity = found.stat.identity();
    let check =
        |path: PathBuf| async move { spawn_blocking(move || check_same(&path, identity)).await? };
    check(found.abs.clone()).await?;
    let tag = store
        .blobs()
        .add_path_with_opts(AddPathOptions {
            path: found.abs.clone(),
            format: BlobFormat::Raw,
            mode: ImportMode::TryReference,
        })
        .temp_tag()
        .await
        .with_context(|| format!("hashing {}", found.path))?;
    check(found.abs.clone()).await?;
    Ok(tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    #[test]
    fn a_walk_keeps_files_lists_folders_and_says_what_it_left_out() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        for file in [
            "Cargo.toml",
            "src/main.rs",
            "target/debug/app",
            "assets/a.png",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file).unwrap();
        }
        fs::create_dir(root.join(".beep-1.partial")).unwrap();
        symlink("/etc/hostname", root.join("link")).unwrap();
        let rules = Rules::load(root).unwrap();
        let walked = walk(root, &rules, 10).unwrap();
        let files: Vec<_> = walked.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["Cargo.toml", "assets/a.png", "src/main.rs"]);
        let mut dirs: Vec<_> = walked
            .dirs
            .iter()
            .map(|d| d.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();
        dirs.sort();
        assert_eq!(dirs, ["", "assets", "src"]);
        let mut skipped: Vec<_> = walked
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), <&str>::from(s.rule)))
            .collect();
        skipped.sort();
        assert_eq!(skipped, [("link", "link"), ("target", "default")]);
        assert_eq!(walked.files[0].stat.size, "Cargo.toml".len() as u64);

        let few = walk(root, &rules, 1).unwrap();
        assert_eq!(few.skipped.len(), 1);
        assert!(few.skipped_more);
    }
}
