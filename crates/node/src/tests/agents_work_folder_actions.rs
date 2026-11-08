use super::*;
use std::fs;
fn root(temp: &tempfile::TempDir) -> WorkRoot {
    WorkRoot::at(&fs::canonicalize(temp.path()).unwrap()).unwrap()
}

#[test]
fn creates_renames_copies_moves_and_deletes_without_overwriting() {
    let t = tempfile::tempdir().unwrap();
    let r = root(&t);
    edit(&r, &FileAction::CreateFolder { path: "src".into() }).unwrap();
    edit(
        &r,
        &FileAction::CreateFile {
            path: "src/a".into(),
        },
    )
    .unwrap();
    fs::write(t.path().join("src/a"), "hello").unwrap();
    edit(
        &r,
        &FileAction::Rename {
            path: "src/a".into(),
            to: "src/b".into(),
        },
    )
    .unwrap();
    edit(
        &r,
        &FileAction::Copy {
            paths: vec!["src/b".into()],
            folder: "".into(),
        },
    )
    .unwrap();
    assert_eq!(fs::read(t.path().join("b")).unwrap(), b"hello");
    assert!(
        edit(
            &r,
            &FileAction::Move {
                paths: vec!["b".into()],
                folder: "src".into()
            }
        )
        .is_err()
    );
    edit(
        &r,
        &FileAction::Rename {
            path: "b".into(),
            to: "c".into(),
        },
    )
    .unwrap();
    edit(
        &r,
        &FileAction::Move {
            paths: vec!["c".into()],
            folder: "src".into(),
        },
    )
    .unwrap();
    edit(
        &r,
        &FileAction::Delete {
            paths: vec!["src".into(), "src/b".into()],
        },
    )
    .unwrap();
    assert!(!t.path().join("src").exists());
}

#[test]
fn recursive_copy_keeps_contents_and_does_not_repeat_selected_children() {
    let t = tempfile::tempdir().unwrap();
    let r = root(&t);
    fs::create_dir_all(t.path().join("src/nested")).unwrap();
    fs::create_dir(t.path().join("dest")).unwrap();
    fs::write(t.path().join("src/nested/a"), b"contents").unwrap();
    edit(
        &r,
        &FileAction::Copy {
            paths: vec!["src/nested/a".into(), "src".into()],
            folder: "dest".into(),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(t.path().join("dest/src/nested/a")).unwrap(),
        b"contents"
    );
    assert!(
        edit(
            &r,
            &FileAction::Copy {
                paths: vec!["src".into()],
                folder: "dest".into()
            }
        )
        .is_err()
    );
    assert_eq!(
        fs::read(t.path().join("dest/src/nested/a")).unwrap(),
        b"contents"
    );
}

#[test]
fn refuses_escape_metadata_self_copy_and_batch_collisions() {
    let t = tempfile::tempdir().unwrap();
    let r = root(&t);
    for path in [
        "",
        "..",
        "../a",
        "/a",
        ".git",
        "a/.GIT/config",
        "C:/a",
        "a\\b",
    ] {
        assert!(
            edit(&r, &FileAction::CreateFile { path: path.into() }).is_err(),
            "{path}"
        );
    }
    fs::create_dir(t.path().join("src")).unwrap();
    fs::write(t.path().join("src/a"), "kept").unwrap();
    assert!(
        edit(
            &r,
            &FileAction::Copy {
                paths: vec!["src".into()],
                folder: "src".into()
            }
        )
        .is_err()
    );
    assert!(
        edit(
            &r,
            &FileAction::Copy {
                paths: vec!["src/a".into(), "src/missing".into()],
                folder: "".into()
            }
        )
        .is_err()
    );
    assert!(!t.path().join("a").exists());
}

#[cfg(unix)]
#[test]
fn links_cannot_redirect_mutations_or_copy_reads() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let r = root(&t);
    fs::write(outside.path().join("keep"), "kept").unwrap();
    symlink(outside.path(), t.path().join("link")).unwrap();
    assert!(
        edit(
            &r,
            &FileAction::CreateFile {
                path: "link/new".into()
            }
        )
        .is_err()
    );
    assert!(
        edit(
            &r,
            &FileAction::Delete {
                paths: vec!["link/keep".into()]
            }
        )
        .is_err()
    );
    assert!(
        edit(
            &r,
            &FileAction::Copy {
                paths: vec!["link".into()],
                folder: "".into()
            }
        )
        .is_err()
    );
    edit(
        &r,
        &FileAction::Delete {
            paths: vec!["link".into()],
        },
    )
    .unwrap();
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"kept");
}
