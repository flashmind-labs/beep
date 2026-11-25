//! The worktrees of the repository holding a folder: the list the apps'
//! worktree menu shows (`list`), and whether a path lies in one that a
//! session's repository lists (`listed_in`), which opens it to reads like a
//! session's own folder. Read-only: nothing here changes a checkout.

use super::found::{Memo, common_dir, git_dir};
use super::work_diff::git;
use crate::files::paths;
use anyhow::Result;
use beep_proto::{SessionId, node::agent::Worktree};
use std::{
    convert::Infallible,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::SystemTime,
};

/// Characters of a commit hash shown.
const SHORT_HEAD: usize = 7;
/// The most repositories whose worktrees are kept for [`listed_in`].
const MAX_REPOSITORIES: usize = 256;

/// The worktree folders git listed, by the repository file whose stamp
/// they were listed with. See [`listed_paths`].
static LISTED: LazyLock<Memo<Vec<PathBuf>>> = LazyLock::new(|| Memo::new(MAX_REPOSITORIES));

/// The worktrees in the output of `git worktree list --porcelain`, the
/// main checkout first. A bare repository is not one.
pub(super) fn parse(text: &str) -> Vec<Worktree> {
    let mut found = Vec::new();

    for block in text.split("\n\n") {
        let mut tree = Worktree {
            path: String::new(),
            branch: None,
            head: String::new(),
            own: false,
            created: false,
            created_by: Vec::new(),
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

/// The checkout a worktree at `path` was made from: the part before a
/// `.worktrees` or `.claude/worktrees` folder, where Beep and Claude Code
/// make them. None for any other path. Reads no file system.
pub(super) fn checkout_of(path: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = path.components().collect();
    let is = |at: usize, name: &str| parts.get(at).is_some_and(|part| part.as_os_str() == name);
    // The folder's own name is not enough: a worktree's name must follow.
    let at = (0..parts.len()).find(|&at| {
        (is(at, ".worktrees") && parts.len() > at + 1)
            || (is(at, ".claude") && is(at + 1, "worktrees") && parts.len() > at + 2)
    })?;
    let checkout: PathBuf = parts[..at].iter().collect();
    checkout.parent().is_some().then_some(checkout)
}

/// The repository a session folder that is gone was a worktree of: its
/// [`checkout_of`] when that holds a `.git` now. Every path check asks this
/// of each session folder, so it reads the file system, never git.
pub(super) fn checkout_of_gone(folder: &Path) -> Option<PathBuf> {
    if folder.exists() {
        return None;
    }

    let checkout = checkout_of(folder)?;
    checkout.join(".git").exists().then_some(checkout)
}

/// The worktrees of the repository holding `folder`, already resolved and
/// in reach; the one holding `folder` is marked `own`. When `folder` is a
/// removed worktree, those of its main checkout, which is `own`. None when
/// `folder` is not in a repository. The ones in `created` (a session's
/// `SessionInfo::worktrees`) are marked `created`, and `makers` (every session's) say which
/// sessions created each. Blocks on git and the
/// file system.
pub(super) fn list(
    folder: &Path,
    created: &[String],
    makers: &[(SessionId, Vec<String>)],
) -> Result<Vec<Worktree>> {
    if let Some(checkout) = checkout_of_gone(folder) {
        return list(&checkout, created, makers);
    }

    let text = match git(folder, &["worktree", "list", "--porcelain"]) {
        Ok(text) => text,
        Err(e) if format!("{e:#}").contains("not a git repository") => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut trees = parse(&text);
    // Git lists the main checkout first; only linked worktrees are made.
    let main = trees.first().map(|tree| tree.path.clone());
    // A synced `.git` lists the worktrees of other machines too.
    trees.retain(|tree| Path::new(&tree.path).is_dir());

    // The innermost one holding the folder: a worktree may sit inside the
    // main checkout.
    let own = trees
        .iter()
        .enumerate()
        .filter_map(|(at, tree)| Some((at, paths::canonicalize(&tree.path).ok()?)))
        .filter(|(_, path)| folder.starts_with(path))
        .max_by_key(|(_, path)| path.as_os_str().len())
        .map(|(at, _)| at);
    if let Some(tree) = own.and_then(|at| trees.get_mut(at)) {
        tree.own = true;
    }

    let created: Vec<PathBuf> = created
        .iter()
        .map(|path| paths::canonicalize(path).unwrap_or_else(|_| path.into()))
        .collect();

    for tree in trees.iter_mut().filter(|tree| Some(&tree.path) != main.as_ref()) {
        let path = paths::canonicalize(&tree.path).unwrap_or_else(|_| tree.path.clone().into());
        tree.created = created.contains(&path);
        tree.created_by = makers
            .iter()
            .filter(|(_, made)| {
                made.iter().any(|made| {
                    paths::canonicalize(made).unwrap_or_else(|_| made.into()) == path
                })
            })
            .map(|(session, _)| session.clone())
            .collect();
    }

    Ok(trees)
}

/// The linked worktrees of the repository holding `folder` that one of
/// `commands` names by its folder's name and that were made at or after
/// `since`, as the `commondir` file git writes once when it adds one says.
/// The main checkout has no `commondir`. Blocks on git and the file system.
pub(super) fn made_since(folder: &Path, since: SystemTime, commands: &[String]) -> Vec<String> {
    let Ok(trees) = list(folder, &[], &[]) else {
        return Vec::new();
    };

    trees
        .into_iter()
        .map(|tree| tree.path)
        .filter(|tree| {
            Path::new(tree)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| commands.iter().any(|command| names(command, name)))
        })
        .filter(|tree| {
            git_dir(Path::new(tree))
                .and_then(|git| fs::metadata(git.join("commondir")).ok())
                .and_then(|meta| meta.modified().ok())
                .is_some_and(|made| made >= since)
        })
        .collect()
}


/// Whether `real`, already resolved, lies in a worktree that git lists for
/// the repository of one of `folders`. Every path check outside the
/// sessions' folders asks this, so git is asked again only once a
/// repository's worktrees change. Blocks on git and the file system.
pub(super) fn listed_in(folders: &[PathBuf], real: &Path) -> bool {
    let mut asked: Vec<PathBuf> = Vec::new();

    for folder in folders {
        let Ok(folder) = paths::canonicalize(folder) else {
            continue;
        };

        if asked.contains(&folder) {
            continue;
        }

        // A synced `.git` lists the worktrees of other machines too.
        let held = listed_paths(&folder)
            .iter()
            .filter(|path| path.is_dir())
            .filter_map(|path| paths::canonicalize(path).ok())
            // A worktree at `/` would open the whole disk.
            .filter(|path| path.parent().is_some())
            .any(|path| real.starts_with(path));
        asked.push(folder);

        if held {
            return true;
        }
    }

    false
}

/// The folders git lists as worktrees of the repository holding `folder`,
/// kept while its `worktrees` folder, or its git folder before it has one,
/// keeps its stamp: adding or removing a worktree changes it. A worktree
/// moved in place is seen once another one is added or removed. Empty, and
/// git is not run, when no `.git` is at or above `folder`; a repository
/// git cannot read lists nothing until its files change.
fn listed_paths(folder: &Path) -> Vec<PathBuf> {
    let Some(common) = common_dir(folder) else {
        return Vec::new();
    };
    // A worktree's `commondir` climbs with `..`; resolved, every worktree
    // of a repository keys the same entry.
    let common = paths::canonicalize(&common).unwrap_or(common);
    let trees = common.join("worktrees");
    let stamped = if trees.is_dir() { trees } else { common };

    let Ok(paths) = LISTED.read(&stamped, || {
        let listed = git(folder, &["worktree", "list", "--porcelain"])
            .map(|text| parse(&text))
            .unwrap_or_default();
        Ok::<_, Infallible>(listed.into_iter().map(|tree| tree.path.into()).collect())
    });
    paths
}

/// Whether `command` names `name` as a whole part of a path.
fn names(command: &str, name: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, '/' | '\\' | '"' | '\''))
        .any(|part| part == name)
}

#[cfg(test)]
pub(in crate::agents) mod tests {
    use super::*;
    use std::fs;
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
            list(&paths::canonicalize(dir.path()).unwrap(), &[], &[])
                .unwrap()
                .is_empty()
        );
    }

    /// A repository with a commit and a worktree outside it.
    fn repo_with_worktree() -> (TempDir, PathBuf, PathBuf) {
        let dir = TempDir::new().unwrap();
        let base = paths::canonicalize(dir.path()).unwrap();
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
        let trees = list(&main.join("src"), &[], &[]).unwrap();
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
    fn a_worktree_made_since_a_time_is_found_and_marked_as_created() {
        let before = SystemTime::now() - std::time::Duration::from_secs(5);
        let (_dir, main, other) = repo_with_worktree();
        let other = other.to_str().unwrap().to_owned();
        let added = ["git worktree add -b side ../other".to_owned()];
        assert_eq!(made_since(&main, before, &added), vec![other.clone()]);
        let elsewhere = ["git worktree add ../another".to_owned()];
        assert!(made_since(&main, before, &elsewhere).is_empty());
        let later = SystemTime::now() + std::time::Duration::from_secs(60);
        assert!(made_since(&main, later, &added).is_empty());

        let created = |made: &[String]| {
            list(&main, made, &[])
                .unwrap()
                .into_iter()
                .filter(|tree| tree.created)
                .map(|tree| tree.path)
                .collect::<Vec<_>>()
        };
        assert_eq!(created(std::slice::from_ref(&other)), vec![other.clone()]);
        assert!(created(&[main.to_str().unwrap().to_owned()]).is_empty());
        assert!(created(&[]).is_empty());
    }

    #[test]
    fn a_list_asked_for_no_session_says_which_sessions_created_each_worktree() {
        let (_dir, main, other) = repo_with_worktree();
        let other = other.to_str().unwrap().to_owned();
        let makers = [
            (SessionId::from("a"), vec![other.clone()]),
            (SessionId::from("b"), vec![other.clone()]),
            (SessionId::from("c"), Vec::new()),
        ];
        let trees = list(&main, &[], &makers).unwrap();
        let made = trees.iter().find(|tree| tree.path == other).unwrap();

        assert_eq!(made.created_by, [SessionId::from("a"), SessionId::from("b")]);
        assert!(trees.iter().filter(|tree| tree.path != other).all(|tree| tree.created_by.is_empty()));
    }

    #[test]
    fn only_the_two_worktree_folders_name_a_checkout() {
        let of = |path: &str| checkout_of(Path::new(path));
        assert_eq!(of("/srv/project/.worktrees/x"), Some("/srv/project".into()));
        assert_eq!(of("/srv/p/.worktrees/x/src"), Some("/srv/p".into()));
        assert_eq!(of("/srv/p/.claude/worktrees/x"), Some("/srv/p".into()));
        assert_eq!(of("/srv/p/.worktrees"), None);
        assert_eq!(of("/srv/p/.claude/worktrees"), None);
        assert_eq!(of("/srv/p/.claude/x"), None);
        assert_eq!(of("/srv/p/trees/x"), None);
        assert_eq!(of("/.worktrees/x"), None);
    }

    /// A repository whose worktree in `dir` was made and then removed.
    pub(in crate::agents) fn repo_with_removed_worktree(dir: &str) -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let main = paths::canonicalize(tmp.path()).unwrap().join("project");
        fs::create_dir(&main).unwrap();
        git_in(&main, &["init", "-q", "-b", "main"]);
        git_in(&main, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let gone = main.join(dir).join("x");
        let at = gone.to_str().unwrap();
        git_in(&main, &["worktree", "add", "-q", "-b", "side", at]);
        git_in(&main, &["worktree", "remove", at]);
        (tmp, main, gone)
    }

    #[test]
    fn a_removed_worktree_lists_its_main_checkout_as_own() {
        for dir in [".worktrees", ".claude/worktrees"] {
            let (_tmp, main, gone) = repo_with_removed_worktree(dir);
            let trees = list(&gone, &[], &[]).unwrap();
            let own: Vec<_> = trees.iter().filter(|tree| tree.own).collect();
            assert_eq!(own.len(), 1, "{dir}");
            assert_eq!(own[0].path, main.to_str().unwrap());
        }
    }

    #[test]
    fn a_removed_folder_outside_the_worktree_names_lists_nothing() {
        let (_tmp, main, _gone) = repo_with_removed_worktree(".worktrees");
        assert!(list(&main.join("elsewhere/x"), &[], &[]).is_err());
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
        let stranger = paths::canonicalize(stranger).unwrap();
        assert!(!listed_in(&session, &stranger));
        assert!(!listed_in(&session, Path::new("/etc")));
        // A folder beside the worktree, not in it.
        assert!(!listed_in(&session, other.parent().unwrap()));
    }

    #[test]
    fn git_is_asked_again_only_once_the_worktrees_change() {
        let (dir, main, other) = repo_with_worktree();
        let session = [main.clone()];
        assert!(listed_in(&session, &other));

        // Git cannot read the repository now, but nothing it lists changed:
        // rewritten in place, `HEAD` leaves both folders' stamps alone.
        let head = main.join(".git/HEAD");
        let saved = fs::read(&head).unwrap();
        fs::write(&head, "broken").unwrap();
        assert!(listed_in(&session, &other));

        fs::write(&head, saved).unwrap();
        let third = paths::canonicalize(dir.path()).unwrap().join("third");
        let at = third.to_str().unwrap();
        git_in(&main, &["worktree", "add", "-q", "-b", "third", at]);
        assert!(listed_in(&session, &third));
    }
}
