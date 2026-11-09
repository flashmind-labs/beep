use super::*;

#[test]
fn only_what_means_the_same_everywhere_travels() {
    for (path, want) in [
        (".git/HEAD", Some(Kind::Head)),
        (".git/shallow", Some(Kind::Shared)),
        (".git/config", Some(Kind::Once)),
        (".git/info/exclude", Some(Kind::Once)),
        (".git/refs/heads/a/b", Some(Kind::Ref)),
        (".git/refs/stash", Some(Kind::Ref)),
        (
            ".git/objects/ab/0123456789012345678901234567890123456f",
            Some(Kind::Object),
        ),
        (".git/objects/pack/pack-0a1b.pack", Some(Kind::Object)),
        (".git/objects/pack/pack-0a1b.idx", Some(Kind::Object)),
        // Temporary, a lock, or a cache that names this machine's packs.
        (".git/objects/ab/tmp_obj_x", None),
        (".git/objects/pack/tmp_pack_x", None),
        (".git/objects/pack/.tmp-1-pack-0a1b.pack", None),
        (".git/objects/pack/pack-0a1b.keep", None),
        (".git/objects/pack/pack-0a1b.bitmap", None),
        (".git/objects/pack/multi-pack-index", None),
        (".git/objects/info/commit-graph", None),
        (".git/objects/info/alternates", None),
        (".git/refs/heads/main.lock", None),
        (".git/refs/heads/a.lock/b", None),
        (".git/refs/bisect/bad", None),
        (".git/index", None),
        (".git/index.lock", None),
        (".git/packed-refs", None),
        (".git/FETCH_HEAD", None),
        (".git/ORIG_HEAD", None),
        (".git/MERGE_HEAD", None),
        (".git/logs/HEAD", None),
        (".git/worktrees/x/HEAD", None),
        (".git/modules/x/HEAD", None),
        (".git/hooks/pre-commit", None),
        ("HEAD", None),
    ] {
        assert_eq!(kind(path), want, "{path}");
    }
}

#[test]
fn a_batch_owns_the_index_lock_and_never_removes_another_operations_lock() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::create_dir(dir.path().join(GIT)).unwrap();
    let checkout = Checkout::read(dir.path());
    let guard = WriteGuard::acquire(dir.path(), Some(checkout)).unwrap();
    assert!(is_busy(dir.path()));
    assert!(WriteGuard::acquire(dir.path(), Some(checkout)).is_err());
    assert!(dir.path().join(GIT).join("index.lock").exists());
    drop(guard);
    assert!(!is_busy(dir.path()));
    fs::write(dir.path().join(GIT).join("index.lock"), "another operation").unwrap();
    assert!(WriteGuard::acquire(dir.path(), Some(checkout)).is_err());
    assert_eq!(
        fs::read_to_string(dir.path().join(GIT).join("index.lock")).unwrap(),
        "another operation"
    );
}

#[test]
fn object_names_of_either_length_read_back() {
    for hex in ["0a".repeat(20), "0a".repeat(32)] {
        assert_eq!(hex.parse::<Oid>().ok().unwrap().to_string(), hex);
    }

    assert_eq!("0a".parse::<Oid>().ok(), None);
    assert_eq!("zz".repeat(20).parse::<Oid>().ok(), None);
}

#[test]
fn a_conflict_ref_is_named_from_its_branch_and_back() {
    let copy = conflict_ref(".git/refs/heads/a/b", "01020304").unwrap();
    assert_eq!(copy, ".git/refs/beep-conflicts/heads/a/b-01020304");
    assert!(is_ref(&copy));
    assert_eq!(original_ref(&copy).as_deref(), Some(".git/refs/heads/a/b"));
    assert_eq!(conflict_ref(".git/HEAD", "01020304"), None);
    assert_eq!(original_ref(".git/refs/heads/a-01020304"), None);
}
