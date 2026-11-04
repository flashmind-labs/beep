//! The list: recording scanned files, merging peers, and planning the
//! work that brings the folder in line.

use super::state::{Seen, State};
use crate::sync::{
    map::{Entry, FileMeta, Map, Order, Version, valid_path},
    rules::Rules,
    scan::{Found, MAX_FILES, Stat},
};
use std::path::Path;

/// One file to bring in line with the list.
#[derive(Debug)]
pub(super) struct Work {
    pub(super) path: String,
    /// The entry it is brought to.
    pub(super) version: Version,
    /// `None` to delete it.
    pub(super) file: Option<FileMeta>,
    /// What the last scan saw there.
    pub(super) expected: Option<Stat>,
}

/// Records a file the scan hashed.
pub(super) fn record(state: &mut State, found: &Found, hash: [u8; 32], prior: Option<Seen>) {
    let meta = FileMeta {
        hash,
        size: found.stat.size,
        mtime_ms: found.stat.mtime_ms(),
        executable: found.stat.executable,
    };
    let version = match prior {
        // Touched, not changed.
        Some(prior) if prior.hash == hash => prior.version,
        Some(prior) => {
            // The list may hold the content seen here under a newer version:
            // a concurrent edit this content won was merged into it. The
            // edit builds on that, so it does not lose to it again.
            let base = match state.saved.map.get(&found.path) {
                Some(entry)
                    if entry.file.as_ref().is_some_and(|f| f.hash == prior.hash)
                        && entry.version.compare(&prior.version) == Order::Newer =>
                {
                    entry.version.clone()
                }
                _ => prior.version,
            };
            state.saved.map.edit(&found.path, &base, Some(meta))
        }
        None => match state.saved.map.get(&found.path) {
            // The same file arrived some other way; nothing changed.
            Some(entry) if entry.file.as_ref().is_some_and(|f| f.hash == hash) => {
                entry.version.clone()
            }
            // New here: concurrent with whatever the list has, so the two
            // are kept apart if they differ.
            _ => state
                .saved
                .map
                .edit(&found.path, &Version::default(), Some(meta)),
        },
    };
    state.set_seen(
        &found.path,
        Seen {
            stat: found.stat,
            hash,
            version,
        },
    );
}

/// Where the content at `path` should move: a path the list wants that
/// content at, that is not on disk and that the rules keep.
pub(super) fn move_target(state: &State, rules: &Rules, path: &str) -> Option<String> {
    let seen = state.saved.seen.get(path)?;
    state
        .saved
        .map
        .entries()
        .filter(|entry| entry.path != path)
        .filter(|entry| entry.file.as_ref().is_some_and(|f| f.hash == seen.hash))
        .filter(|entry| !rules.skips_any(Path::new(&entry.path), false))
        .find(|entry| !state.saved.seen.contains_key(&entry.path))
        .map(|entry| entry.path.clone())
}

/// What merging a peer's entries did.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Merged {
    pub(super) changed: bool,
    /// Entries left out: a path no peer may name, or a new file past the
    /// file limit.
    pub(super) refused: usize,
}

/// Merges a peer's `entries` into `map`.
pub(super) fn merge_peer(map: &mut Map, entries: &[Entry]) -> Merged {
    let mut live = map.entries().filter(|entry| entry.file.is_some()).count();
    let mut merged = Merged {
        changed: false,
        refused: 0,
    };
    let is_live = |map: &Map, path: &str| map.get(path).is_some_and(|known| known.file.is_some());
    for entry in entries {
        let was_live = is_live(map, &entry.path);
        if !valid_path(&entry.path) || (entry.file.is_some() && !was_live && live >= MAX_FILES) {
            merged.refused += 1;
            continue;
        }
        for path in map.merge(entry) {
            merged.changed = true;
            let now_live = is_live(map, &path);
            let before = path == entry.path && was_live;
            live = live + usize::from(now_live) - usize::from(before);
        }
    }
    merged
}

/// What it takes to bring the folder in line with the list: the paths
/// whose entry is not what the folder holds, leaving out paths the rules
/// skip here. Moves to conflict copies come first.
pub(super) fn plan(state: &State, rules: &Rules) -> Vec<Work> {
    let mut work: Vec<Work> = state
        .saved
        .map
        .entries()
        // A list saved before peers' paths were checked may hold others.
        .filter(|entry| valid_path(&entry.path))
        .filter(|entry| !rules.skips_any(Path::new(&entry.path), false))
        .filter_map(|entry| {
            let seen = state.saved.seen.get(&entry.path);
            match (&entry.file, seen) {
                (Some(file), Some(seen)) if seen.hash == file.hash => None,
                (None, None) => None,
                _ => Some(Work {
                    path: entry.path.clone(),
                    version: entry.version.clone(),
                    file: entry.file.clone(),
                    expected: seen.map(|seen| seen.stat),
                }),
            }
        })
        .collect();
    work.sort_by_key(|item| item.expected.is_none());
    work
}
