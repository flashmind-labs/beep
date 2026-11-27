use super::*;
use std::{cell::Cell, convert::Infallible, fs::File};

fn counted(memo: &Memo<String>, path: &Path, reads: &Cell<u32>) -> String {
    let read = || {
        reads.set(reads.get() + 1);
        Ok::<_, Infallible>(fs::read_to_string(path).expect("the file"))
    };
    memo.read(path, read).expect("infallible")
}

#[test]
fn an_unchanged_file_is_read_once_and_a_changed_one_again() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = root.path().join("a.jsonl");
    fs::write(&path, "one").expect("a file");
    let memo = Memo::new(8);
    let reads = Cell::new(0);
    assert_eq!(counted(&memo, &path, &reads), "one");
    assert_eq!(counted(&memo, &path, &reads), "one");
    assert_eq!(reads.get(), 1);
    fs::write(&path, "three").expect("a longer file");
    assert_eq!(counted(&memo, &path, &reads), "three");
    assert_eq!(reads.get(), 2);
}

#[test]
fn a_failed_read_is_not_kept() {
    let root = tempfile::tempdir().expect("a temp folder");
    let path = root.path().join("a.db");
    fs::write(&path, "x").expect("a file");
    let memo: Memo<u32> = Memo::new(8);
    assert_eq!(memo.read(&path, || Err("locked")), Err("locked"));
    assert_eq!(memo.read(&path, || Ok::<_, &str>(7)), Ok(7));
    assert_eq!(memo.read(&path, || Err("locked")), Ok(7));
}

#[test]
fn only_a_non_empty_wal_file_counts_as_a_change() {
    let root = tempfile::tempdir().expect("a temp folder");
    let db = root.path().join("s.db");
    let wal = root.path().join("s.db-wal");
    fs::write(&db, "x").expect("a database");
    let memo: Memo<u32> = Memo::new(8);
    assert_eq!(memo.read_database(&db, || Ok::<_, Infallible>(1)), Ok(1));
    assert_eq!(memo.read_database(&db, || Ok::<_, Infallible>(2)), Ok(1));
    File::create(&wal).expect("an empty wal");
    assert_eq!(memo.read_database(&db, || Ok::<_, Infallible>(3)), Ok(1));
    fs::write(&wal, "frames").expect("a non-empty wal");
    assert_eq!(memo.read_database(&db, || Ok::<_, Infallible>(4)), Ok(4));
}
