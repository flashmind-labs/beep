//! The checkout a folder is in, read with gitoxide: its main folder (a worktree's
//! is the checkout it was added to, a subfolder's the top) and its `origin`
//! remote, and when its checked-out commit was made.
//!
//! Folder listings ask this of hundreds of folders at a time, so answers are
//! kept while the files git read for them keep their stamps: a commit, a
//! branch switch or a new remote rewrites one of them. A folder with no
//! `.git` above it is in no checkout, and the repository is not opened.

use crate::files::paths;
use crate::lock;
use beep_proto::node::agent::WorkPlace;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant, SystemTime},
};

/// How long an answer is kept while its files keep their stamps, for what
/// they do not show, such as a remote set in an included config file.
const KEPT_FOR: Duration = Duration::from_secs(10 * 60);
/// The most folders kept for each question; a full cache is emptied.
const MAX_KEPT: usize = 1024;

static CHECKOUTS: LazyLock<Kept<Option<Checkout>>> = LazyLock::new(Kept::default);
static COMMITS: LazyLock<Kept<Option<u64>>> = LazyLock::new(Kept::default);
static HEADS: LazyLock<Kept<Option<String>>> = LazyLock::new(Kept::default);

/// A git checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Checkout {
    /// Its main folder.
    pub(crate) root: PathBuf,
    /// Its `origin` remote; empty when it has none.
    pub(crate) origin: String,
}

/// The checkout `dir` is in; `None` when it is in none or git is missing.
/// Kept while the checkout's git folder stays where it is and its config
/// keeps its stamp.
pub(crate) fn checkout(dir: &Path) -> Option<Checkout> {
    let files = GitFiles::above(dir)?;
    let stamp = files.stamp(&[files.common.join("config")]);
    CHECKOUTS.get(dir, stamp, || ask_checkout(dir))
}

fn ask_checkout(dir: &Path) -> Option<Checkout> {
    let (_, root) = folders(dir)?;
    let origin = origin(&root);
    Some(Checkout { root, origin })
}

/// Repositories opened with gitoxide, by their git folder, with the stamp
/// of the config they were opened with.
static REPOSITORIES: LazyLock<Mutex<HashMap<PathBuf, (Stamp, gix::ThreadSafeRepository)>>> =
    LazyLock::new(Mutex::default);
/// The most repositories kept open; a full cache is emptied.
const MAX_REPOSITORIES: usize = 64;

/// The repository `dir` is in, opened once and kept while its config keeps
/// its stamp: its pack indexes load once, not on every read. Refs are read
/// from disk each time and new packs are found when an object is missing.
/// It ignores the environment and global config, as sync's git does.
pub(crate) fn repository(dir: &Path) -> Option<gix::Repository> {
    let files = GitFiles::above(dir)?;
    let git = paths::canonicalize(&files.git).unwrap_or_else(|_| files.git.clone());
    let stamp = files.stamp(&[files.common.join("config")]);

    if let Some((kept, repo)) = lock(&REPOSITORIES).get(&git)
        && *kept == stamp
    {
        return Some(repo.to_thread_local());
    }

    let repo = gix::ThreadSafeRepository::open_opts(&git, gix::open::Options::isolated()).ok()?;
    let mut repositories = lock(&REPOSITORIES);

    if repositories.len() >= MAX_REPOSITORIES {
        repositories.clear();
    }

    repositories.insert(git, (stamp, repo.clone()));
    Some(repo.to_thread_local())
}

/// The branch `origin/HEAD` names, such as `main`, if `origin` names one.
pub(crate) fn default_branch(repo: &gix::Repository) -> Option<String> {
    let head = repo.try_find_reference("refs/remotes/origin/HEAD").ok()??;
    let name = head.target().try_name()?.shorten().to_string();
    name.strip_prefix("origin/").map(str::to_owned)
}

/// The top folder of the work tree `dir` is in, and of the repository's main
/// checkout.
fn folders(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    folders_of(&repository(dir)?, dir)
}

/// Both resolved, as git prints them.
fn folders_of(repo: &gix::Repository, dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let real = |path: &Path| paths::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let top = real(repo.workdir()?);
    let main = main_folder(dir, &top, &real(repo.common_dir()));
    Some((top, main))
}

/// The checkout `dir` is in, as a session's place: whether it is a linked
/// worktree of its repository, and the branch. `None` when `dir` is in no
/// checkout. Reads the repository's files: call it from a blocking task,
/// and not for every update.
pub(crate) fn place(dir: &Path) -> Option<WorkPlace> {
    GitFiles::above(dir)?;
    let repo = repository(dir)?;
    let (top, main) = folders_of(&repo, dir)?;
    let real_top = paths::canonicalize(&top).unwrap_or_else(|_| top.clone());
    let branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string());
    Some(WorkPlace {
        linked: real_top != main,
        folder: top.to_string_lossy().into_owned(),
        main: main.to_string_lossy().into_owned(),
        branch,
        origin: origin(&main),
    })
}

/// The `origin` remote of the checkout at `root`; empty when it has none.
pub(crate) fn origin(root: &Path) -> String {
    repository(root)
        .and_then(|repo| {
            let remote = repo.find_remote("origin").ok()?;
            let url = remote.url(gix::remote::Direction::Fetch)?;
            Some(url.to_bstring().to_string())
        })
        .unwrap_or_default()
}

/// When the commit checked out at `root` was made, Unix seconds; `None`
/// before the first commit or when git is missing. Kept while `HEAD` and the
/// ref it names keep their stamps.
pub(crate) fn last_commit(root: &Path) -> Option<u64> {
    let files = GitFiles::above(root)?;
    let stamp = files.stamp(&files.head_files());
    COMMITS.get(root, stamp, || {
        let seconds = repository(root)?.head_commit().ok()?.committer().ok()?.seconds();
        u64::try_from(seconds).ok()
    })
}

/// The commit checked out at `root`, as `ask` names it, kept while `HEAD`
/// and the ref it names keep their stamps. File sync asks this several times
/// in each scan. A missing answer is asked again each time: the ref can
/// arrive before its commit does.
pub(crate) fn head(root: &Path, ask: impl FnOnce() -> Option<String>) -> Option<String> {
    let Some(files) = GitFiles::above(root) else {
        return ask();
    };
    let stamp = files.stamp(&files.head_files());
    HEADS.found(root, stamp, ask)
}

/// The checkout's main folder: the one holding the shared `.git`, else the
/// top of this one (a submodule or a separate git dir).
fn main_folder(dir: &Path, top: &Path, common: &Path) -> PathBuf {
    let common = if common.is_absolute() {
        common.to_owned()
    } else {
        // Relative to `dir`, spelled as the user knows it.
        match paths::canonicalize(dir.join(common)) {
            Ok(path) => path,
            Err(_) => return top.to_owned(),
        }
    };

    match common.file_name() {
        Some(name) if name == ".git" => common
            .parent()
            .map_or_else(|| top.to_owned(), Path::to_owned),
        _ => top.to_owned(),
    }
}


/// The git folder the worktrees of the repository holding `dir` share,
/// found without running git; `None` when `dir` is in no checkout.
pub(crate) fn common_dir(dir: &Path) -> Option<PathBuf> {
    GitFiles::above(dir).map(|files| files.common)
}

/// The git folder of the checkout holding `dir`: `.git`, or for a linked
/// worktree the folder its `.git` file names.
pub(crate) fn git_dir(dir: &Path) -> Option<PathBuf> {
    GitFiles::above(dir).map(|files| files.git)
}

/// Where git keeps a checkout's files, found without running git.
#[derive(Debug)]
struct GitFiles {
    /// This checkout's git folder: `.git`, or the one a `.git` file names.
    git: PathBuf,
    /// The folder shared by its worktrees, with the config and refs.
    common: PathBuf,
}

impl GitFiles {
    /// The files of the nearest `.git` at or above `dir`; `None` when there
    /// is none, so `dir` is in no checkout.
    fn above(dir: &Path) -> Option<Self> {
        let (top, dot) = dir
            .ancestors()
            .map(|top| (top, top.join(".git")))
            .find(|(_, dot)| fs::symlink_metadata(dot).is_ok())?;
        let git = match fs::read_to_string(&dot) {
            // A worktree or a submodule: `gitdir: <folder>`.
            Ok(text) => text
                .trim()
                .strip_prefix("gitdir:")
                .map_or_else(|| dot.clone(), |named| top.join(named.trim())),
            Err(_) => dot,
        };
        let common = match fs::read_to_string(git.join("commondir")) {
            Ok(text) => git.join(text.trim()),
            Err(_) => git.clone(),
        };
        Some(Self { git, common })
    }

    /// `HEAD`, the ref it names and the shared ref stores: a commit, a branch
    /// switch or a fetch rewrites one of them.
    fn head_files(&self) -> Vec<PathBuf> {
        let head = self.git.join("HEAD");
        let named = fs::read_to_string(&head).ok().and_then(|text| {
            text.trim()
                .strip_prefix("ref:")
                .map(|name| self.common.join(name.trim()))
        });
        [
            Some(head),
            named,
            Some(self.common.join("packed-refs")),
            Some(self.common.join("reftable").join("tables.list")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn stamp(&self, files: &[PathBuf]) -> Stamp {
        Stamp {
            git: self.git.clone(),
            files: files.iter().map(|file| FileStamp::of(file)).collect(),
        }
    }
}

/// Where the git folder an answer came from is, and the state of its files.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    git: PathBuf,
    files: Vec<Option<FileStamp>>,
}

/// A file's time, size and, where there is one, inode: git writes refs and
/// `HEAD` by renaming a new file over the old, so the inode changes even
/// within one tick of a coarse clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
    inode: u64,
}

impl FileStamp {
    fn of(file: &Path) -> Option<Self> {
        let meta = fs::metadata(file).ok()?;
        #[cfg(unix)]
        let inode = std::os::unix::fs::MetadataExt::ino(&meta);
        #[cfg(not(unix))]
        let inode = 0;
        Some(Self {
            modified: meta.modified().ok(),
            len: meta.len(),
            inode,
        })
    }
}

/// Answers by folder, each with the stamp of the files it came from. A
/// failed answer is kept too, so a folder git cannot read is asked again
/// only once its files change.
struct Kept<T> {
    entries: Mutex<HashMap<PathBuf, Entry<T>>>,
}

struct Entry<T> {
    stamp: Stamp,
    at: Instant,
    value: T,
}

impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self {
            entries: Mutex::default(),
        }
    }
}

impl<T: Clone> Kept<T> {
    /// The answer kept for `key` while its files still have `stamp`, else
    /// what `ask` says now, kept. The lock is not held while `ask` runs.
    fn get(&self, key: &Path, stamp: Stamp, ask: impl FnOnce() -> T) -> T {
        if let Some(value) = self.kept(key, &stamp) {
            return value;
        }

        let value = ask();
        self.keep(key, stamp, value.clone());
        value
    }

    fn kept(&self, key: &Path, stamp: &Stamp) -> Option<T> {
        lock(&self.entries)
            .get(key)
            .filter(|entry| entry.stamp == *stamp && entry.at.elapsed() < KEPT_FOR)
            .map(|entry| entry.value.clone())
    }

    fn keep(&self, key: &Path, stamp: Stamp, value: T) {
        let mut entries = lock(&self.entries);

        if entries.len() >= MAX_KEPT {
            entries.clear();
        }

        entries.insert(
            key.to_owned(),
            Entry {
                stamp,
                at: Instant::now(),
                value,
            },
        );
    }
}

impl<T: Clone> Kept<Option<T>> {
    /// As [`Kept::get`], keeping only an answer that found something.
    fn found(&self, key: &Path, stamp: Stamp, ask: impl FnOnce() -> Option<T>) -> Option<T> {
        if let Some(Some(value)) = self.kept(key, &stamp) {
            return Some(value);
        }

        let value = ask()?;
        self.keep(key, stamp, Some(value.clone()));
        Some(value)
    }
}

#[cfg(test)]
#[path = "../tests/files_git_read.rs"]
pub(crate) mod tests;
