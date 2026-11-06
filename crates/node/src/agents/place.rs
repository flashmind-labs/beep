//! Where a session works: the git checkout of the folder its agent records,
//! and whether that is a linked worktree. Asked of git once per folder and
//! kept while its `HEAD` and config files keep their times, so a session's
//! updates do not each start git.

use super::found::place;
use beep_proto::node::agent::WorkPlace;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime},
};

/// How long an answer is kept while its files say nothing changed: a folder
/// can still move under the same name.
const KEPT: Duration = Duration::from_secs(10 * 60);
/// How long a folder in no checkout is taken to be in none: `git init` or
/// a clone there writes no file this looks at.
const KEPT_NONE: Duration = Duration::from_secs(60);
/// The most folders kept; a full cache is emptied.
const MAX_KEPT: usize = 256;

/// What the lookup of a folder found, and when.
struct Answer {
    at: Instant,
    place: Option<WorkPlace>,
    /// The times of the files a branch switch or a new remote writes.
    stamp: Vec<Option<SystemTime>>,
}

static CACHE: LazyLock<Mutex<HashMap<PathBuf, Answer>>> = LazyLock::new(Mutex::default);

/// The place of the folder `dir`, or of its nearest folder that exists
/// when a file or a removed folder is named; `None` outside any checkout.
/// Blocks (git): run it with `spawn_blocking`.
pub(crate) fn of_folder(dir: &Path) -> Option<WorkPlace> {
    let dir = existing(dir)?;
    let now = Instant::now();
    let kept = lock().get(&dir).and_then(|answer| {
        let age = now.duration_since(answer.at);
        let fresh = match &answer.place {
            Some(place) => age < KEPT && stamp(place) == answer.stamp,
            None => age < KEPT_NONE,
        };
        fresh.then(|| answer.place.clone())
    });
    if let Some(place) = kept {
        return place;
    }
    let found = place(&dir);
    let stamp = found.as_ref().map(stamp).unwrap_or_default();
    let mut cache = lock();
    if cache.len() >= MAX_KEPT {
        cache.clear();
    }
    cache.insert(
        dir,
        Answer {
            at: now,
            place: found.clone(),
            stamp,
        },
    );
    found
}

/// The modification times of the checkout's `HEAD` and config files, which
/// git rewrites when the branch or a remote changes. A linked worktree or a
/// submodule keeps its `HEAD` in the folder its `.git` file names.
fn stamp(place: &WorkPlace) -> Vec<Option<SystemTime>> {
    let top = Path::new(&place.folder).join(".git");
    let git = match fs::read_to_string(&top) {
        Ok(text) => text
            .trim()
            .strip_prefix("gitdir:")
            .map(|dir| Path::new(&place.folder).join(dir.trim()))
            .unwrap_or(top),
        Err(_) => top,
    };
    let common = Path::new(&place.main).join(".git");
    [git.join("HEAD"), git.join("config"), common.join("config")]
        .iter()
        .map(|file| fs::metadata(file).and_then(|meta| meta.modified()).ok())
        .collect()
}

fn lock() -> MutexGuard<'static, HashMap<PathBuf, Answer>> {
    // The map holds plain values, so a panic elsewhere leaves it usable.
    CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `path` when it is a folder, else its nearest folder above that exists.
fn existing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| dir.is_dir())
        .map(Path::to_owned)
}

/// The folder a tool call works in: the one its input names (`workdir`,
/// `cwd`), else the folder of the file an edit changes. A relative one is
/// taken from `base`. `None` when it names none.
pub(crate) fn call_folder(
    base: &Path,
    input: Option<&serde_json::Value>,
    edited: Option<&Path>,
) -> Option<PathBuf> {
    let named = input.and_then(|input| {
        ["workdir", "cwd", "working_directory"]
            .iter()
            .find_map(|key| input[key].as_str())
            .filter(|dir| !dir.is_empty())
    });
    let dir = match (named, edited) {
        (Some(dir), _) => base.join(dir),
        (None, Some(file)) if file.is_absolute() => file.parent()?.to_owned(),
        _ => return None,
    };
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::checkout::tests::{has_git, run};
    use crate::files::paths;
    use serde_json::json;
    use std::fs;

    #[test]
    fn a_linked_worktree_is_told_from_the_main_checkout() {
        if !has_git() {
            eprintln!("skipped: no git");
            return;
        }
        let root = tempfile::tempdir().expect("a temp folder");
        fs::create_dir_all(root.path().join("proj")).expect("a folder");
        let repo = paths::canonicalize(root.path().join("proj")).expect("a real path");
        run(&repo, &["init", "-q", "-b", "main"]);
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
        run(
            &repo,
            &["worktree", "add", "-q", "-b", "fix", ".worktrees/fix"],
        );

        let main = of_folder(&repo.join("src/deep")).expect("a place");
        assert!(!main.linked);
        assert_eq!(main.branch.as_deref(), Some("main"));
        assert_eq!(main.main, repo.to_string_lossy());

        let tree = repo.join(".worktrees/fix");
        // A file that is not there yet counts for its folder.
        let linked = of_folder(&tree.join("new/file.rs")).expect("a place");
        assert!(linked.linked);
        assert_eq!(linked.branch.as_deref(), Some("fix"));
        assert_eq!(linked.main, repo.to_string_lossy());
        assert_eq!(linked.origin, "https://example.com/org/proj.git");
        assert_eq!(linked.folder, tree.to_string_lossy());

        // A branch switch shows at once, though the answer is kept.
        run(&tree, &["switch", "-q", "-c", "next"]);
        let switched = of_folder(&tree).expect("a place");
        assert_eq!(switched.branch.as_deref(), Some("next"));
    }

    #[test]
    fn a_call_names_its_folder_by_workdir_or_the_file_it_edits() {
        let base = Path::new("/srv/project");
        let workdir = json!({"cmd": "ls", "workdir": "/srv/project/.worktrees/fix"});
        assert_eq!(
            call_folder(base, Some(&workdir), None),
            Some(PathBuf::from("/srv/project/.worktrees/fix"))
        );
        let relative = json!({"workdir": "sub"});
        assert_eq!(
            call_folder(base, Some(&relative), None),
            Some(PathBuf::from("/srv/project/sub"))
        );
        let file = Path::new("/srv/project/.worktrees/fix/src/lib.rs");
        assert_eq!(
            call_folder(base, None, Some(file)),
            Some(PathBuf::from("/srv/project/.worktrees/fix/src"))
        );
        assert_eq!(call_folder(base, Some(&json!({"cmd": "ls"})), None), None);
    }
}
