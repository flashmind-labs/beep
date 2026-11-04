//! The git checkouts under the home folder, found on disk rather than in
//! session logs, so a checkout no agent ran in still counts. Only main
//! checkouts: the walk stops at a repository's top, so nested repositories
//! and worktrees inside it are not looked at, and a worktree elsewhere
//! (whose `.git` is a file) is left out. Home is walked once, on the first
//! start, and what it found is saved in the node's folder; later checkouts
//! come from the agents' logs.

use super::git::{self, Checkout};
use anyhow::{Context, Result};
use ignore::{DirEntry, WalkBuilder};
use std::{
    fs,
    io::ErrorKind,
    path::Path,
    time::{Duration, Instant},
};

/// How deep under home the walk goes: `~/code/org/repo` is 3.
const MAX_DEPTH: usize = 4;
/// The most checkouts one walk gives.
const MAX_CHECKOUTS: usize = 400;
/// How long one walk, with its remote lookups, may take.
const WALK_FOR: Duration = Duration::from_secs(3);
/// Folders never worth walking into.
const PRUNED: &[&str] = &["node_modules", "target", "Library"];
/// The checkouts the first walk found, in the node's folder.
const FILE: &str = "checkouts.json";
/// Written first, then renamed over `FILE`.
const TEMP: &str = "checkouts.json.tmp";

/// The checkouts saved in `dir`. When none are saved yet, as on the first
/// start, walks `home` for them and saves what it found.
pub(crate) fn saved_checkouts(home: &Path, dir: &Path) -> Vec<Checkout> {
    let path = dir.join(FILE);
    match fs::read_to_string(&path) {
        Ok(text) => {
            return serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("ignored {}: {e}", path.display());
                Vec::new()
            });
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => {
            log::warn!("could not read {}: {e}", path.display());
            return Vec::new();
        }
    }
    let found = checkouts(home);
    if let Err(e) = save(dir, &found) {
        log::warn!("the checkouts found were not saved: {e:#}");
    }
    found
}

/// Writes `found` down in `dir`, whole or not at all.
fn save(dir: &Path, found: &[Checkout]) -> Result<()> {
    let (temp, path) = (dir.join(TEMP), dir.join(FILE));
    let text = serde_json::to_string(found).context("writing the checkouts")?;
    fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, &path).with_context(|| format!("writing {}", path.display()))
}

/// The main checkouts under `home`, with their `origin` remotes.
fn checkouts(home: &Path) -> Vec<Checkout> {
    let deadline = Instant::now() + WALK_FOR;
    let walk = WalkBuilder::new(home)
        .max_depth(Some(MAX_DEPTH))
        .parents(false)
        .filter_entry(walked)
        .build();
    let mut out = Vec::new();
    for entry in walk {
        if out.len() >= MAX_CHECKOUTS || Instant::now() > deadline {
            break;
        }
        let Ok(entry) = entry else { continue };
        let root = entry.path();
        if entry.depth() > 0 && root.join(".git").is_dir() {
            out.push(Checkout {
                root: root.to_owned(),
                origin: git::origin(root),
            });
        }
    }
    out
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
    use crate::found::git::tests::run;
    use std::{fs, process::Command};

    #[test]
    fn main_checkouts_are_found_and_the_rest_left_out() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("skipped: no git");
            return;
        }
        let temp = tempfile::tempdir().expect("a temp folder");
        let home = temp.path().canonicalize().expect("a real path");
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

        let mut found = checkouts(&home);
        found.sort_by(|a, b| a.root.cmp(&b.root));
        let roots: Vec<&Path> = found.iter().map(|one| one.root.as_path()).collect();
        assert_eq!(roots, [nasty.as_path(), notes.as_path()]);
        assert_eq!(found[0].origin, "https://example.com/me/nasty.git");
        assert_eq!(found[1].origin, "");
    }

    #[test]
    fn a_later_start_reads_the_saved_checkouts_without_walking() {
        let temp = tempfile::tempdir().expect("a temp folder");
        let (home, dir) = (temp.path().join("home"), temp.path().join("node"));
        let repo = home.join("code/app");
        fs::create_dir_all(repo.join(".git")).expect("a checkout");
        fs::create_dir_all(&dir).expect("the node's folder");

        let first = saved_checkouts(&home, &dir);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].root, repo);
        fs::remove_dir_all(&home).expect("home gone");
        assert_eq!(saved_checkouts(&home, &dir), first);
    }
}
