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
