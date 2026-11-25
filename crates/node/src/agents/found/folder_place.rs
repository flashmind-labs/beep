//! Where a session works: the git checkout of the folder its agent records,
//! and whether that is a linked worktree. Asked of git once per folder and
//! kept while its `HEAD` and config files keep their times, so a session's
//! updates do not each start git.

use super::place;
use beep_proto::node::agent::WorkPlace;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant, SystemTime},
};

/// How long an answer is kept while its files say nothing changed: a folder
/// can still move under the same name.
const KEPT: Duration = Duration::from_secs(10 * 60);
/// How long a folder in no checkout is taken to be in none: `git init` or
/// a clone there writes no file this looks at.
const KEPT_NONE: Duration = Duration::from_secs(60);
/// The most folders kept; a full cache is emptied.
const MAX_KEPT: usize = 1024;

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
    let kept = crate::lock(&CACHE).get(&dir).and_then(|answer| {
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
    let mut cache = crate::lock(&CACHE);

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

/// `path` when it is a folder, else its nearest folder above that exists.
fn existing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| dir.is_dir())
        .map(Path::to_owned)
}

/// The folder a tool call works in: the one its input names (`workdir`,
/// `cwd`), else the folder of the file an edit changes. A relative one is
/// taken from `base`. `None` when it names none, or names a `machine`: Beep's
/// remote tools work in a folder on that machine.
pub(crate) fn call_folder(
    base: &Path,
    input: Option<&serde_json::Value>,
    edited: Option<&Path>,
) -> Option<PathBuf> {
    if input.is_some_and(|input| input["machine"].as_str().is_some_and(|m| !m.is_empty())) {
        return None;
    }

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
#[path = "../../tests/agents_place.rs"]
mod tests;
