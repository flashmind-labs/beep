//! The worktrees of the repository holding a folder: the list the apps'
//! worktree menu shows (`list`), and whether a path lies in one that a
//! session's repository lists (`listed_in`), which opens it to reads like a
//! session's own folder. Read-only: nothing here changes a checkout.

use super::work_diff::git;
use anyhow::Result;
use beep_proto::node::agent::Worktree;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Characters of a commit hash shown.
const SHORT_HEAD: usize = 7;

/// The worktrees in the output of `git worktree list --porcelain`, the
/// main checkout first. A bare repository is not one.
fn parse(text: &str) -> Vec<Worktree> {
    let mut found = Vec::new();
    for block in text.split("\n\n") {
        let mut tree = Worktree {
            path: String::new(),
            branch: None,
            head: String::new(),
            own: false,
        };
        let mut bare = false;
        for line in block.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                tree.path = path.to_owned();
            } else if let Some(head) = line.strip_prefix("HEAD ") {
                tree.head = head.chars().take(SHORT_HEAD).collect();
            } else if let Some(branch) = line.strip_prefix("branch ") {
                let name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
                tree.branch = Some(name.to_owned());
            } else if line == "bare" {
                bare = true;
            }
        }
        if !tree.path.is_empty() && !bare {
            found.push(tree);
        }
    }
    found
}

/// The worktrees of the repository holding `folder`, already resolved and
/// in reach; the one holding `folder` is marked `own`. None when `folder`
/// is not in a repository. Blocks on git and the file system.
pub(super) fn list(folder: &Path) -> Result<Vec<Worktree>> {
    let text = match git(folder, &["worktree", "list", "--porcelain"]) {
        Ok(text) => text,
        Err(e) if format!("{e:#}").contains("not a git repository") => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut trees = parse(&text);
    // The innermost one holding the folder: a worktree may sit inside the
    // main checkout.
    let own = trees
        .iter()
        .enumerate()
        .filter_map(|(at, tree)| Some((at, fs::canonicalize(&tree.path).ok()?)))
        .filter(|(_, path)| folder.starts_with(path))
        .max_by_key(|(_, path)| path.as_os_str().len())
        .map(|(at, _)| at);
    if let Some(tree) = own.and_then(|at| trees.get_mut(at)) {
        tree.own = true;
    }
    Ok(trees)
}

/// Whether `real`, already resolved, lies in a worktree that git lists for
/// the repository of one of `folders`, asked of git now. Blocks on git and
/// the file system.
pub(super) fn listed_in(folders: &[PathBuf], real: &Path) -> bool {
    let mut asked: Vec<PathBuf> = Vec::new();
    for folder in folders {
        let Ok(folder) = fs::canonicalize(folder) else {
            continue;
        };
        if asked.contains(&folder) {
            continue;
        }
        let Ok(trees) = list(&folder) else {
            asked.push(folder);
            continue;
        };
        asked.push(folder);
        let held = trees
            .iter()
            .filter_map(|tree| fs::canonicalize(&tree.path).ok())
            // A worktree at `/` would open the whole disk.
            .filter(|path| path.parent().is_some())
            .any(|path| real.starts_with(path));
        if held {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git_in(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "{args:?}: {out:?}");
    }

    #[test]
    fn the_porcelain_list_reads_branches_and_detached_heads() {
        let text = "worktree /srv/project\nHEAD 0123456789abcdef\nbranch refs/heads/main\n\n\
                    worktree /srv/project/.worktrees/fix\nHEAD fedcba9876543210\ndetached\n\n\
                    worktree /srv/bare\nbare\n";
        let found = parse(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].branch.as_deref(), Some("main"));
        assert_eq!(found[0].head, "0123456");
        assert_eq!(found[1].path, "/srv/project/.worktrees/fix");
        assert_eq!(found[1].branch, None);
    }

    #[test]
    fn a_folder_that_is_no_repository_has_no_worktrees() {
        let dir = TempDir::new().unwrap();
        assert!(
            list(&fs::canonicalize(dir.path()).unwrap())
                .unwrap()
                .is_empty()
        );
    }

    /// A repository with a commit and a worktree outside it.
    fn repo_with_worktree() -> (TempDir, PathBuf, PathBuf) {
        let dir = TempDir::new().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let main = base.join("main");
        let other = base.join("other");
        fs::create_dir(&main).unwrap();
        git_in(&main, &["init", "-q", "-b", "main"]);
        git_in(&main, &["commit", "-q", "--allow-empty", "-m", "first"]);
        git_in(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "side",
                other.to_str().unwrap(),
            ],
        );
        (dir, main, other)
    }

    #[test]
    fn the_list_marks_the_worktree_holding_the_folder() {
        let (_dir, main, other) = repo_with_worktree();
        fs::create_dir(main.join("src")).unwrap();
        let trees = list(&main.join("src")).unwrap();
        assert_eq!(trees.len(), 2);
        let own: Vec<_> = trees.iter().filter(|tree| tree.own).collect();
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].path, main.to_str().unwrap());
        assert!(
            trees
                .iter()
                .any(|tree| tree.path == other.to_str().unwrap())
        );
    }

    #[test]
    fn a_listed_worktree_is_reachable_and_another_folder_is_not() {
        let (dir, main, other) = repo_with_worktree();
        fs::write(other.join("notes.txt"), "x").unwrap();
        let session = [main];
        assert!(listed_in(&session, &other));
        assert!(listed_in(&session, &other.join("notes.txt")));
        let stranger = dir.path().join("stranger");
        fs::create_dir(&stranger).unwrap();
        let stranger = fs::canonicalize(stranger).unwrap();
        assert!(!listed_in(&session, &stranger));
        assert!(!listed_in(&session, Path::new("/etc")));
        // A folder beside the worktree, not in it.
        assert!(!listed_in(&session, other.parent().unwrap()));
    }
}
