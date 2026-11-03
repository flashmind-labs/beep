//! The git checkouts under the home folder, found on disk rather than in
//! session logs, so a checkout no agent ran in still counts. Only main
//! checkouts: the walk stops at a repository's top, so nested repositories
//! and worktrees inside it are not looked at, and a worktree elsewhere
//! (whose `.git` is a file) is left out.

use super::git::{self, Checkout};
use ignore::{DirEntry, WalkBuilder};
use std::{
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

/// The main checkouts under `home`, with their `origin` remotes.
pub(crate) fn checkouts(home: &Path) -> Vec<Checkout> {
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
}
