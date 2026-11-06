//! The main git checkouts under the search folder (`search` in `node.toml`,
//! home by default), found on disk so a checkout no agent ran in still counts.
//! The last walk's list is saved and served from `saved_checkouts` while the
//! next start's `walk` looks again.

use super::git::{self, Checkout};
use crate::{link::note, lock};
use anyhow::{Context, Result};
use ignore::{DirEntry, WalkBuilder, WalkState};
use std::{
    fs,
    io::ErrorKind,
    path::Path,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// How deep under home the walk goes: `~/code/org/repo` is 3.
const MAX_DEPTH: usize = 4;
/// The most checkouts one walk gives.
const MAX_CHECKOUTS: usize = 400;
/// How long the walk, with its remote lookups, may take. It runs in the
/// background, and a large home takes minutes to walk.
const WALK_FOR: Duration = Duration::from_secs(5 * 60);
/// How deep a quick look again goes: `~/org/repo` is 2.
const QUICK_DEPTH: usize = 3;
/// How long a quick look again may take: the Add a project sheet waits for it.
const QUICK_FOR: Duration = Duration::from_secs(8);
/// Folders never worth walking into.
const PRUNED: &[&str] = &["node_modules", "target", "Library"];
/// The checkouts the last whole walk found, in the node's folder.
const FILE: &str = "checkouts.json";

/// What one walk found.
pub(crate) struct Walk {
    pub(crate) found: Vec<Checkout>,
    /// False when it stopped at `WALK_FOR` or `MAX_CHECKOUTS`: checkouts may
    /// be missing, so it is not saved.
    pub(crate) whole: bool,
}

/// Drops the checkouts saved in `dir`, so nothing stands for them until the
/// next walk ends.
pub(crate) fn forget_checkouts(dir: &Path) -> Result<()> {
    let path = dir.join(FILE);
    match fs::remove_file(&path) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

/// The checkouts saved in `dir`; `None` when none can be read, as on the
/// first start.
pub(crate) fn saved_checkouts(dir: &Path) -> Option<Vec<Checkout>> {
    let path = dir.join(FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!("could not read {}: {e}", path.display());
            return None;
        }
    };
    serde_json::from_str(&text)
        .inspect_err(|e| log::warn!("ignored {}: {e}", path.display()))
        .ok()
}

/// Walks `root` for checkouts and saves them in `dir` when the walk was whole.
pub(crate) fn walk(root: &Path, dir: &Path) -> Walk {
    let walk = checkouts(root, MAX_DEPTH, WALK_FOR);
    if walk.whole
        && let Err(e) = save(dir, &walk.found)
    {
        log::warn!("the checkouts found were not saved: {e:#}");
    }
    walk
}

/// `known` with what a quick, shallow walk of `root` finds added, and those no longer checkouts
/// dropped; saved in `dir`. Checkouts deeper than it goes stay as the last whole walk left them.
pub(crate) fn rescan(root: &Path, dir: &Path, known: &[Checkout]) -> Vec<Checkout> {
    let walk = checkouts(root, QUICK_DEPTH, QUICK_FOR);
    let mut all: Vec<Checkout> = known
        .iter()
        .filter(|one| one.root.join(".git").is_dir())
        .cloned()
        .collect();
    for one in walk.found {
        if !all.iter().any(|known| known.root == one.root) {
            all.push(one);
        }
    }
    if let Err(e) = save(dir, &all) {
        log::warn!("the checkouts found were not saved: {e:#}");
    }
    all
}

/// Writes `found` down in `dir`, whole or not at all.
fn save(dir: &Path, found: &[Checkout]) -> Result<()> {
    note::write_json(&dir.join(FILE), &found)
}

/// The main checkouts under `home`, with their `origin` remotes, walked
/// on several threads: a large home takes minutes on one. A worktree
/// outside its checkout, whose `.git` is a file, is left out.
fn checkouts(home: &Path, depth: usize, time: Duration) -> Walk {
    let deadline = Instant::now() + time;
    let found = Mutex::new(Vec::new());
    let cut = AtomicBool::new(false);
    WalkBuilder::new(home)
        .max_depth(Some(depth))
        .parents(false)
        .filter_entry(walked)
        .build_parallel()
        .run(|| {
            Box::new(|entry| {
                if Instant::now() > deadline {
                    cut.store(true, Ordering::Relaxed);
                    return WalkState::Quit;
                }
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                let root = entry.path();
                if entry.depth() == 0 || !root.join(".git").is_dir() {
                    return WalkState::Continue;
                }
                let checkout = Checkout {
                    root: root.to_owned(),
                    origin: git::origin(root),
                };
                let mut found = lock(&found);
                found.push(checkout);
                if found.len() >= MAX_CHECKOUTS {
                    cut.store(true, Ordering::Relaxed);
                    WalkState::Quit
                } else {
                    // Nothing below a checkout's top is looked at.
                    WalkState::Skip
                }
            })
        });
    Walk {
        found: found.into_inner().unwrap_or_else(PoisonError::into_inner),
        whole: !cut.into_inner(),
    }
}

/// Whether the walk goes into `entry`: a folder, not pruned, and not
/// inside a checkout (home itself may be one, for dotfiles).
fn walked(entry: &DirEntry) -> bool {
    if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return false;
    }
    if PRUNED.iter().any(|name| entry.file_name() == *name) {
        return false;
    }
    entry.depth() < 2
        || entry
            .path()
            .parent()
            .is_none_or(|parent| !parent.join(".git").exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::found::git::tests::{has_git, run};
    use crate::files::paths;
    use std::{fs, path::PathBuf};

    #[test]
    fn main_checkouts_are_found_and_the_rest_left_out() {
        if !has_git() {
            eprintln!("skipped: no git");
            return;
        }
        let temp = tempfile::tempdir().expect("a temp folder");
        let home = paths::canonicalize(temp.path()).expect("a real path");
        let repo = |rel: &str| {
            let dir = home.join(rel);
            fs::create_dir_all(&dir).expect("a folder");
            run(&dir, &["init", "-q"]);
            dir
        };
        let nasty = repo("code/games/nasty");
        run(
            &nasty,
            &[
                "remote",
                "add",
                "origin",
                "https://example.com/me/nasty.git",
            ],
        );
        run(&nasty, &["commit", "-q", "--allow-empty", "-m", "start"]);
        run(&nasty, &["worktree", "add", "-q", "../nasty-fix"]);
        let notes = repo("notes");
        repo("notes/vendored");
        repo("code/web/node_modules/dep");
        repo(".config/plugin");
        repo("a/b/c/d/too-deep");

        let mut found = checkouts(&home, MAX_DEPTH, WALK_FOR).found;
        found.sort_by(|a, b| a.root.cmp(&b.root));
        let roots: Vec<&Path> = found.iter().map(|one| one.root.as_path()).collect();
        assert_eq!(roots, [nasty.as_path(), notes.as_path()]);
        assert_eq!(found[0].origin, "https://example.com/me/nasty.git");
        assert_eq!(found[1].origin, "");
    }

    #[test]
    fn a_whole_walk_is_saved_for_the_next_start() {
        let temp = tempfile::tempdir().expect("a temp folder");
        let (home, dir) = (temp.path().join("home"), temp.path().join("node"));
        let repo = home.join("code/app");
        fs::create_dir_all(repo.join(".git")).expect("a checkout");
        fs::create_dir_all(&dir).expect("the node's folder");

        assert_eq!(saved_checkouts(&dir), None);
        let first = walk(&home, &dir);
        assert!(first.whole);
        assert_eq!(first.found.len(), 1);
        assert_eq!(first.found[0].root, repo);
        fs::remove_dir_all(&home).expect("home gone");
        assert_eq!(saved_checkouts(&dir), Some(first.found));
    }

    #[test]
    fn a_rescan_adds_new_checkouts_and_drops_gone_ones() {
        let temp = tempfile::tempdir().expect("a temp folder");
        let (home, dir) = (temp.path().join("home"), temp.path().join("node"));
        let (kept, gone, new) = (home.join("code/app"), home.join("old"), home.join("ops"));
        for repo in [&kept, &gone] {
            fs::create_dir_all(repo.join(".git")).expect("a checkout");
        }
        fs::create_dir_all(&dir).expect("the node's folder");
        let known = walk(&home, &dir).found;
        assert_eq!(known.len(), 2);

        fs::remove_dir_all(&gone).expect("a checkout removed");
        fs::create_dir_all(new.join(".git")).expect("a checkout made");
        let mut roots: Vec<PathBuf> = rescan(&home, &dir, &known)
            .into_iter()
            .map(|one| one.root)
            .collect();
        roots.sort();
        assert_eq!(roots, [kept, new]);
        assert_eq!(saved_checkouts(&dir).map(|saved| saved.len()), Some(2));
    }
}
