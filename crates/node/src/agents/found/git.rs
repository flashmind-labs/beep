//! The checkout a folder is in, asked of git: its main folder (a worktree's
//! is the checkout it was added to, a subfolder's the top) and its `origin`
//! remote.

use crate::agents::work_folder::git;
use beep_proto::node::agent::WorkPlace;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A git checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Checkout {
    /// Its main folder.
    pub(crate) root: PathBuf,
    /// Its `origin` remote; empty when it has none.
    pub(crate) origin: String,
}

/// The checkout `dir` is in; `None` when it is in none or git is missing.
pub(crate) fn checkout(dir: &Path) -> Option<Checkout> {
    let said = git(dir, &["rev-parse", "--show-toplevel", "--git-common-dir"])?;
    let mut lines = said.lines();
    let top = PathBuf::from(lines.next()?.trim());
    let common = PathBuf::from(lines.next()?.trim());
    let root = main_folder(dir, &top, &common);
    let origin = origin(&root);
    Some(Checkout { root, origin })
}

/// The checkout `dir` is in, as a session's place: whether it is a linked
/// worktree of its repository, and the branch. `None` when `dir` is in no
/// checkout or git is missing. Three runs of git: call it from a blocking
/// task, and not for every update.
pub(crate) fn place(dir: &Path) -> Option<WorkPlace> {
    let said = git(dir, &["rev-parse", "--show-toplevel", "--git-common-dir"])?;
    let mut lines = said.lines();
    let top = PathBuf::from(lines.next()?.trim());
    let common = PathBuf::from(lines.next()?.trim());
    let main = main_folder(dir, &top, &common);
    let real_top = top.canonicalize().unwrap_or_else(|_| top.clone());
    let branch = git(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    Some(WorkPlace {
        linked: real_top != main,
        folder: top.to_string_lossy().into_owned(),
        main: main.to_string_lossy().into_owned(),
        branch,
        origin: origin(&main),
    })
}

/// The `origin` remote of the checkout at `root`; empty when it has none.
pub(super) fn origin(root: &Path) -> String {
    git(root, &["remote", "get-url", "origin"])
        .map(|url| url.trim().to_owned())
        .unwrap_or_default()
}

/// When the commit checked out at `root` was made, Unix seconds; `None`
/// before the first commit or when git is missing.
pub(crate) fn last_commit(root: &Path) -> Option<u64> {
    git(root, &["log", "-1", "--format=%ct"])?
        .trim()
        .parse()
        .ok()
}

/// The checkout's main folder: the one holding the shared `.git`, else the
/// top of this one (a submodule or a separate git dir).
fn main_folder(dir: &Path, top: &Path, common: &Path) -> PathBuf {
    let common = if common.is_absolute() {
        common.to_owned()
    } else {
        // Relative to `dir`, as git prints it there.
        match dir.join(common).canonicalize() {
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

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::fs;

    pub(in crate::agents::found) use crate::agents::checkout::tests::{has_git, run};

    #[test]
    fn a_main_folder_comes_from_the_shared_git_dir() {
        let top = Path::new("/home/user/proj/.worktrees/fix");
        assert_eq!(
            main_folder(top, top, Path::new("/home/user/proj/.git")),
            Path::new("/home/user/proj")
        );
        assert_eq!(
            main_folder(top, top, Path::new("/home/user/proj/.git/modules/sub")),
            top
        );
    }

    #[test]
    fn subfolders_and_worktrees_lead_to_the_checkout() {
        if !has_git() {
            eprintln!("skipped: no git");
            return;
        }
        let root = tempfile::tempdir().expect("a temp folder");
        fs::create_dir_all(root.path().join("proj")).expect("a folder");
        let repo = root
            .path()
            .join("proj")
            .canonicalize()
            .expect("a real path");
        run(&repo, &["init", "-q"]);
        run(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://example.com/org/proj.git",
            ],
        );
        run(&repo, &["commit", "-q", "--allow-empty", "-m", "start"]);
        fs::create_dir_all(repo.join("src/deep")).expect("folders");
        run(&repo, &["worktree", "add", "-q", ".worktrees/fix"]);
        for dir in [
            repo.clone(),
            repo.join("src/deep"),
            repo.join(".worktrees/fix"),
        ] {
            let found = checkout(&dir).expect("a checkout");
            assert_eq!(found.root, repo, "from {}", dir.display());
            assert_eq!(found.origin, "https://example.com/org/proj.git");
        }
        let plain = root.path().join("plain");
        fs::create_dir_all(&plain).expect("a folder");
        // A temp folder may sit inside a checkout on some machines; only a
        // folder git calls outside any is checked.
        if git(&plain, &["rev-parse", "--show-toplevel"]).is_none() {
            assert_eq!(checkout(&plain), None);
        }
    }
}
