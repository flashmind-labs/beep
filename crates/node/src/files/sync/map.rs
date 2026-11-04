//! One project's shared file list (`Map`): per path a hash, size and mtime,
//! or a tombstone, each with a `Version`. `merge` makes every node's copy
//! converge whatever order entries arrive in.

use super::{
    gitdir,
    rules::{is_own, is_syncthings},
};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeMap, fmt::Write as _};

/// One node's copy of one project's list, made fresh with its state, so a
/// node that lost its state never reuses counters it gave out before.
pub(super) type Replica = u64;

/// Edits per replica.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Version(BTreeMap<Replica, u64>);

/// How one version stands to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Order {
    Same,
    /// It has seen every edit the other has, and more.
    Newer,
    Older,
    Concurrent,
}

impl Version {
    pub(super) fn compare(&self, other: &Self) -> Order {
        let (mut ahead, mut behind) = (false, false);
        for replica in self.0.keys().chain(other.0.keys()) {
            match self.get(*replica).cmp(&other.get(*replica)) {
                Ordering::Greater => ahead = true,
                Ordering::Less => behind = true,
                Ordering::Equal => {}
            }
        }
        match (ahead, behind) {
            (false, false) => Order::Same,
            (true, false) => Order::Newer,
            (false, true) => Order::Older,
            (true, true) => Order::Concurrent,
        }
    }

    pub(super) fn get(&self, replica: Replica) -> u64 {
        self.0.get(&replica).copied().unwrap_or(0)
    }

    /// Every edit either has seen.
    fn max(&self, other: &Self) -> Self {
        let mut out = self.clone();
        for (&replica, &count) in &other.0 {
            let mine = out.0.entry(replica).or_default();
            *mine = (*mine).max(count);
        }
        out
    }

    /// This version with `replica`'s edit `count` on top.
    fn with(&self, replica: Replica, count: u64) -> Self {
        let mut out = self.clone();
        out.0.insert(replica, count);
        out
    }
}

/// A file's content as the list holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FileMeta {
    #[serde(with = "serde_bytes")]
    pub(super) hash: [u8; 32],
    pub(super) size: u64,
    /// Milliseconds since the Unix epoch.
    pub(super) mtime_ms: i64,
    #[serde(default)]
    pub(super) executable: bool,
}

/// One path in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Entry {
    /// Relative to the project folder, parts joined with `/`.
    pub(super) path: String,
    pub(super) version: Version,
    /// `None` for a deleted file.
    pub(super) file: Option<FileMeta>,
}

/// What merging an entry into the list changes.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Merge {
    /// The path's new entry, or `None` when the list keeps its own.
    pub(super) entry: Option<Entry>,
    /// The conflict copy the losing side of two concurrent edits becomes.
    pub(super) copy: Option<Entry>,
}

/// Which of two concurrent entries keeps the path: a file before a
/// tombstone, then the later mtime, then the larger hash, then the executable
/// one, so that no two different files tie and each node keeps its own.
fn rank(entry: &Entry) -> (bool, i64, [u8; 32], bool) {
    match &entry.file {
        Some(file) => (true, file.mtime_ms, file.hash, file.executable),
        None => (false, 0, [0; 32], false),
    }
}

/// The name a losing edit of `path` with content `hash` moves to:
/// `notes (conflict 1a2b3c4d).txt` beside `notes.txt`. Never one inside
/// `.git`, where such a name breaks git: a ref keeps its loser under another
/// ref (`gitdir::conflict_ref`), and the rest of `.git` keeps no loser.
pub(super) fn conflict_path(path: &str, hash: &[u8; 32]) -> Option<String> {
    let tag = hash_tag(hash);
    if gitdir::in_git(path) {
        return gitdir::conflict_ref(path, &tag);
    }
    let name_at = path.rfind('/').map_or(0, |at| at + 1);
    Some(match path[name_at..].rfind('.').filter(|&at| at > 0) {
        Some(dot) => {
            let dot = name_at + dot;
            format!("{} (conflict {tag}){}", &path[..dot], &path[dot..])
        }
        None => format!("{path} (conflict {tag})"),
    })
}

fn hash_tag(hash: &[u8; 32]) -> String {
    let mut tag = String::with_capacity(8);
    for byte in &hash[..4] {
        // Writing to a string cannot fail.
        let _ = write!(tag, "{byte:02x}");
    }
    tag
}

/// The longest path a peer may name.
const MAX_PATH: usize = 4096;

/// Whether a peer may name `path`: plain names joined with `/`, none of
/// them `.git`, in any case, or a name sync or Syncthing keeps for itself.
/// The root's `.git` is the exception, for what `gitdir` lets travel.
pub(super) fn valid_path(path: &str) -> bool {
    if gitdir::in_git(path) {
        return path.len() <= MAX_PATH
            && path.split('/').all(|part| {
                !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', '\0'])
            })
            && gitdir::kind(path).is_some();
    }
    !path.is_empty()
        && path.len() <= MAX_PATH
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(['\\', '\0'])
                && !part.eq_ignore_ascii_case(".git")
                && !is_own(part)
                && !is_syncthings(part)
        })
}

/// Whether `path` is a conflict copy.
pub(super) fn is_conflict(path: &str) -> bool {
    if gitdir::in_git(path) {
        return gitdir::original_ref(path).is_some();
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    name.contains(" (conflict ")
}

/// The file conflict copy `path` stands beside: `notes.txt` for
/// `notes (conflict 1a2b3c4d).txt`. `None` when `path` is not a copy.
pub(super) fn original_of(path: &str) -> Option<String> {
    if gitdir::in_git(path) {
        return gitdir::original_ref(path);
    }
    const MARK: &str = " (conflict ";
    let name_at = path.rfind('/').map_or(0, |at| at + 1);
    let at = name_at + path[name_at..].rfind(MARK)?;
    let tag_at = at + MARK.len();
    let tag = path.get(tag_at..tag_at + 8)?;
    if !tag.bytes().all(|b| b.is_ascii_hexdigit()) || path.get(tag_at + 8..tag_at + 9)? != ")" {
        return None;
    }
    let original = format!("{}{}", &path[..at], &path[tag_at + 9..]);
    (original.len() > name_at).then_some(original)
}

/// Merges `incoming` into `local`, the entry the list has for its path. A
/// version that has seen all of another's edits replaces it. Concurrent
/// edits are both kept: the newer by mtime (a live file before a
/// tombstone, so an edit beats a delete) keeps the path, and the other
/// moves to a conflict copy whose name and version every node derives the
/// same way.
pub(super) fn merge(local: Option<&Entry>, incoming: &Entry) -> Merge {
    let Some(local) = local else {
        return Merge {
            entry: Some(incoming.clone()),
            copy: None,
        };
    };
    match local.version.compare(&incoming.version) {
        Order::Newer => return Merge::default(),
        Order::Same if local == incoming => return Merge::default(),
        Order::Older => {
            return Merge {
                entry: Some(incoming.clone()),
                copy: None,
            };
        }
        // Concurrent, or the same version with other content, which only
        // a broken peer sends: settled the same way on every node.
        _ => {}
    }
    let (winner, loser) = if rank(incoming) > rank(local) {
        (incoming, local)
    } else {
        (local, incoming)
    };
    let entry = Entry {
        path: local.path.clone(),
        version: local.version.max(&incoming.version),
        file: winner.file.clone(),
    };
    let copy = match (&loser.file, &winner.file) {
        (Some(lost), Some(won)) if lost.hash == won.hash => None,
        (Some(lost), _) => conflict_path(&local.path, &lost.hash).map(|path| Entry {
            path,
            version: loser.version.clone(),
            file: Some(lost.clone()),
        }),
        (None, _) => None,
    };
    Merge {
        entry: (entry != *local).then_some(entry),
        copy,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    entry: Entry,
    /// When this list last changed it, in its own order.
    seq: u64,
}

/// More changes than a node makes between two saves.
const LOAD_GAP: u64 = 1 << 32;

/// One node's copy of a project's list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Map {
    replica: Replica,
    /// The highest count this replica gave an edit, or saw for itself.
    clock: u64,
    /// The last change's sequence number.
    seq: u64,
    entries: BTreeMap<String, Stored>,
    /// Paths by the sequence number of their last change.
    #[serde(skip)]
    by_seq: BTreeMap<u64, String>,
}

impl Default for Map {
    fn default() -> Self {
        Self::new(uuid::Uuid::new_v4().as_u64_pair().0)
    }
}

impl Map {
    pub(super) fn new(replica: Replica) -> Self {
        Self {
            replica,
            clock: 0,
            seq: 0,
            entries: BTreeMap::new(),
            by_seq: BTreeMap::new(),
        }
    }

    /// Rebuilds what is not saved, after loading. A crash can load a list
    /// older than what peers already got from it: moving its counts past any
    /// it gave out keeps a peer's cursor from skipping the next changes, and
    /// an old edit from passing for newer than the next ones.
    pub(super) fn loaded(mut self) -> Self {
        self.seq += LOAD_GAP;
        self.clock += LOAD_GAP;
        self.by_seq = self
            .entries
            .iter()
            .map(|(path, stored)| (stored.seq, path.clone()))
            .collect();
        self
    }

    pub(super) fn replica(&self) -> Replica {
        self.replica
    }

    pub(super) fn seq(&self) -> u64 {
        self.seq
    }

    pub(super) fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.get(path).map(|stored| &stored.entry)
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().map(|stored| &stored.entry)
    }

    fn put(&mut self, entry: Entry) {
        self.seq += 1;
        let seq = self.seq;
        let path = entry.path.clone();
        if let Some(old) = self.entries.insert(path.clone(), Stored { entry, seq }) {
            self.by_seq.remove(&old.seq);
        }
        self.by_seq.insert(seq, path);
    }

    /// Merges an entry another node sent, giving the paths that changed.
    pub(super) fn merge(&mut self, incoming: &Entry) -> Vec<String> {
        // Its own counts, seen again after a restore, are never reused.
        self.clock = self.clock.max(incoming.version.get(self.replica));
        let merged = merge(self.get(&incoming.path), incoming);
        let mut changed = Vec::new();
        // The copy first, so a peer reading the changes in order never sees
        // the path settled without it, and replaces the only file it has of
        // the losing edit before it knows where that edit went.
        if let Some(copy) = merged.copy {
            changed.extend(self.merge(&copy));
        }
        if let Some(entry) = merged.entry {
            changed.push(entry.path.clone());
            self.put(entry);
        }
        changed
    }

    /// Records this node's own edit of `path`, made to the content of
    /// version `base`; `None` for a delete. Gives the edit's version, which
    /// the path keeps unless a concurrent edit wins it.
    pub(super) fn edit(&mut self, path: &str, base: &Version, file: Option<FileMeta>) -> Version {
        self.clock += 1;
        let entry = Entry {
            path: path.to_owned(),
            version: base.with(self.replica, self.clock),
            file,
        };
        self.merge(&entry);
        entry.version
    }

    /// The entries changed after `seq`, oldest change first, at most
    /// `limit`, and the sequence number of the last one given.
    pub(super) fn since(&self, seq: u64, limit: usize) -> Changes {
        let mut changes = Changes {
            entries: Vec::new(),
            upto: seq,
        };
        for (&at, path) in self.by_seq.range(seq + 1..).take(limit) {
            if let Some(stored) = self.entries.get(path) {
                changes.entries.push(stored.entry.clone());
            }
            changes.upto = at;
        }
        changes
    }
}

/// A run of changes to send.
#[derive(Debug)]
pub(super) struct Changes {
    pub(super) entries: Vec<Entry>,
    pub(super) upto: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(hash: u8, mtime_ms: i64) -> Option<FileMeta> {
        Some(FileMeta {
            hash: [hash; 32],
            size: 1,
            mtime_ms,
            executable: false,
        })
    }

    fn version(counts: &[(Replica, u64)]) -> Version {
        Version(counts.iter().copied().collect())
    }

    fn entry(path: &str, counts: &[(Replica, u64)], file: Option<FileMeta>) -> Entry {
        Entry {
            path: path.into(),
            version: version(counts),
            file,
        }
    }

    #[test]
    fn concurrent_edits_keep_both() {
        let mine = entry("docs/notes.txt", &[(1, 2)], file(1, 100));
        let theirs = entry("docs/notes.txt", &[(1, 1), (2, 1)], file(2, 200));
        let merged = merge(Some(&mine), &theirs);
        // The later edit keeps the path, with both histories.
        let kept = merged.entry.expect("the path changes");
        assert_eq!(kept.file, theirs.file);
        assert_eq!(kept.version, version(&[(1, 2), (2, 1)]));
        let copy = merged.copy.expect("the other edit is kept");
        assert_eq!(copy.path, "docs/notes (conflict 01010101).txt");
        assert_eq!(copy.file, mine.file);
        assert_eq!(copy.version, mine.version);

        // The other side settles it the same way.
        let other = merge(Some(&theirs), &mine);
        assert_eq!(other.entry, Some(kept));
        assert_eq!(other.copy, Some(copy));
    }

    #[test]
    fn concurrent_edits_inside_git_never_make_a_conflict_copy_there() {
        let mine = entry(".git/refs/heads/main", &[(1, 1)], file(1, 100));
        let theirs = entry(".git/refs/heads/main", &[(2, 1)], file(2, 200));
        let merged = merge(Some(&mine), &theirs);
        assert_eq!(
            merged.entry.expect("the later commit wins").file,
            theirs.file
        );
        let copy = merged.copy.expect("the other commit stays reachable");
        assert_eq!(copy.path, ".git/refs/beep-conflicts/heads/main-01010101");
        assert!(valid_path(&copy.path));
        assert!(is_conflict(&copy.path));
        assert_eq!(
            original_of(&copy.path).as_deref(),
            Some(".git/refs/heads/main")
        );

        let mine = entry(".git/HEAD", &[(1, 1)], file(1, 100));
        let theirs = entry(".git/HEAD", &[(2, 1)], file(2, 200));
        assert_eq!(merge(Some(&mine), &theirs).copy, None);
    }

    #[test]
    fn an_edit_beats_a_concurrent_delete() {
        let edit = entry("a.txt", &[(1, 1), (2, 1)], file(3, 1));
        let delete = entry("a.txt", &[(1, 2)], None);
        let merged = merge(Some(&delete), &edit);
        let kept = merged.entry.expect("the edit wins");
        assert_eq!(kept.file, edit.file);
        assert_eq!(merged.copy, None);
        let merged = merge(Some(&edit), &delete);
        // The edit stays; its version takes in the delete's.
        assert_eq!(merged.entry.expect("the version grows").file, edit.file);
        assert_eq!(merged.copy, None);
    }

    #[test]
    fn tombstones_replace_and_stay() {
        let mut map = Map::new(1);
        map.edit("a.txt", &Version::default(), file(1, 1));
        let base = map.get("a.txt").unwrap().version.clone();
        let deleted = map.edit("a.txt", &base, None);
        assert_eq!(map.get("a.txt").unwrap().version, deleted);
        let tomb = map.get("a.txt").expect("a tombstone stays").clone();
        assert_eq!(tomb.file, None);
        // The old file, sent again by a node that had not seen the delete,
        // does not come back.
        let stale = entry("a.txt", &[(1, 1)], file(1, 1));
        assert!(map.merge(&stale).is_empty());
        assert_eq!(map.get("a.txt"), Some(&tomb));
        // Two deletes made at once settle without a copy.
        let other = entry("a.txt", &[(1, 1), (2, 1)], None);
        let changed = map.merge(&other);
        assert_eq!(changed, ["a.txt"]);
        assert_eq!(map.get("a.txt").unwrap().file, None);
        assert_eq!(map.entries().count(), 1);
    }

    #[test]
    fn two_maps_converge_whatever_order_entries_arrive_in() {
        let mut a = Map::new(1);
        let mut b = Map::new(2);
        a.edit("x", &Version::default(), file(1, 10));
        b.merge(a.get("x").unwrap());
        let base = a.get("x").unwrap().version.clone();
        // Both edit `x` before seeing the other's edit.
        a.edit("x", &base, file(2, 20));
        b.edit("x", &base, file(3, 30));
        b.edit("y", &Version::default(), file(4, 1));
        let from_a = a.since(0, 100).entries;
        let from_b = b.since(0, 100).entries;
        for entry in from_b.iter().rev() {
            a.merge(entry);
        }
        for entry in &from_a {
            b.merge(entry);
        }
        let list = |map: &Map| map.entries().cloned().collect::<Vec<_>>();
        assert_eq!(list(&a), list(&b));
        let paths: Vec<_> = a.entries().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["x", "x (conflict 02020202)", "y"]);
        assert_eq!(a.get("x").unwrap().file, file(3, 30));
    }

    #[test]
    fn changes_come_in_order_and_resume_from_a_cursor() {
        let mut map = Map::new(1);
        for name in ["a", "b", "c"] {
            map.edit(name, &Version::default(), file(1, 1));
        }
        let base = map.get("a").unwrap().version.clone();
        map.edit("a", &base, file(2, 2));
        let first = map.since(0, 2);
        let names: Vec<_> = first.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["b", "c"]);
        let rest = map.since(first.upto, 10);
        assert_eq!(rest.entries.len(), 1);
        assert_eq!(rest.entries[0].path, "a");
        assert_eq!(rest.upto, map.seq());
        assert!(map.since(map.seq(), 10).entries.is_empty());

        let bytes = rmp_serde::to_vec_named(&map).unwrap();
        let loaded = rmp_serde::from_slice::<Map>(&bytes).unwrap().loaded();
        assert_eq!(loaded.since(first.upto, 10).entries, rest.entries);
    }

    #[test]
    fn files_differing_only_in_the_executable_bit_settle_alike() {
        let plain = entry("a", &[(1, 1)], file(1, 5));
        let mut exec = entry("a", &[(2, 1)], file(1, 5));
        if let Some(file) = &mut exec.file {
            file.executable = true;
        }
        let mut one = Map::new(1);
        one.merge(&plain);
        one.merge(&exec);
        let mut other = Map::new(2);
        other.merge(&exec);
        other.merge(&plain);
        assert_eq!(one.get("a"), other.get("a"));
    }

    #[test]
    fn a_peer_gets_a_conflict_copy_before_the_entry_that_settles_it() {
        let mut map = Map::new(1);
        map.merge(&entry("a", &[(1, 1)], file(1, 1)));
        let before = map.seq();
        map.merge(&entry("a", &[(2, 1)], file(2, 2)));
        let first = map.since(before, 1).entries;
        assert_eq!(first[0].path, "a (conflict 01010101)");
    }

    #[test]
    fn a_list_loaded_after_a_crash_reuses_nothing_peers_got() {
        let mut map = Map::new(1);
        map.edit("a", &Version::default(), file(1, 1));
        let saved = rmp_serde::to_vec_named(&map).unwrap();
        // What the crash loses, and a peer already has.
        let lost = map.edit("b", &Version::default(), file(2, 2));
        let cursor = map.seq();

        let mut loaded = rmp_serde::from_slice::<Map>(&saved).unwrap().loaded();
        let again = loaded.edit("b", &Version::default(), file(3, 3));
        assert_eq!(again.compare(&lost), Order::Newer);
        let sent = loaded.since(cursor, 10).entries;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].file, file(3, 3));
    }

    #[test]
    fn a_restored_replica_never_reuses_its_counts() {
        let mut map = Map::new(7);
        map.merge(&entry("a", &[(7, 5)], file(1, 1)));
        map.edit("b", &Version::default(), file(1, 1));
        assert_eq!(map.get("b").unwrap().version, version(&[(7, 6)]));
    }
}
