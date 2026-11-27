use super::*;
use std::fs;
fn rel(path: &str) -> RelPath {
    RelPath::try_from(path).unwrap()
}

fn root(temp: &tempfile::TempDir) -> WorkRoot {
    WorkRoot::at(&fs::canonicalize(temp.path()).unwrap()).unwrap()
}

#[test]
fn creates_renames_copies_moves_and_deletes_without_overwriting() {
    let t = tempfile::tempdir().unwrap();
    let r = root(&t);
    edit(&r, &FileAction::CreateFolder { path: rel("src") }).unwrap();
    edit(&r, &FileAction::CreateFile { path: rel("src/a") }).unwrap();
    fs::write(t.path().join("src/a"), "hello").unwrap();
    edit(
        &r,
        &FileAction::Rename {
            path: rel("src/a"),
            to: rel("src/b"),
        },
    )
    .unwrap();
    edit(
        &r,
        &FileAction::Copy {
            paths: vec![rel("src/b")],
            folder: rel(""),
        },
    )
    .unwrap();
    assert_eq!(fs::read(t.path().join("b")).unwrap(), b"hello");
    assert!(
        edit(
            &r,
            &FileAction::Move {
                paths: vec![rel("b")],
                folder: rel("src")
            }
        )
        .is_err()
    );
    edit(
        &r,
        &FileAction::Rename {
            path: rel("b"),
            to: rel("c"),
        },
    )
    .unwrap();
    edit(
        &r,
        &FileAction::Move {
            paths: vec![rel("c")],
            folder: rel("src"),
        },
    )
    .unwrap();
    edit(
        &r,
        &FileAction::Delete {
            paths: vec![rel("src"), rel("src/b")],
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
            paths: vec![rel("src/nested/a"), rel("src")],
            folder: rel("dest"),
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
                paths: vec![rel("src")],
                folder: rel("dest")
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
    for path in ["..", "../a", "a/../b", "/a", "C:/a", "a\\b"] {
        assert!(RelPath::try_from(path).is_err(), "{path}");
    }
    for path in ["", ".git", "a/.GIT/config"] {
        assert!(
            edit(&r, &FileAction::CreateFile { path: rel(path) }).is_err(),
            "{path}"
        );
    }
    fs::create_dir(t.path().join("src")).unwrap();
    fs::write(t.path().join("src/a"), "kept").unwrap();
    assert!(
        edit(
            &r,
            &FileAction::Copy {
                paths: vec![rel("src")],
                folder: rel("src")
            }
        )
        .is_err()
    );
    assert!(
        edit(
            &r,
            &FileAction::Copy {
                paths: vec![rel("src/a"), rel("src/missing")],
                folder: rel("")
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
                path: rel("link/new")
            }
        )
        .is_err()
    );
    assert!(
        edit(
            &r,
            &FileAction::Delete {
                paths: vec![rel("link/keep")]
            }
        )
        .is_err()
    );
    assert!(
        edit(
            &r,
            &FileAction::Copy {
                paths: vec![rel("link")],
                folder: rel("")
            }
        )
        .is_err()
    );
    edit(
        &r,
        &FileAction::Delete {
            paths: vec![rel("link")],
        },
    )
    .unwrap();
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"kept");
}

#[test]
fn write_replaces_a_file_and_keeps_its_mode() {
    let t = tempfile::tempdir().unwrap();
    let r = root(&t);
    let file = t.path().join("run.sh");
    fs::write(&file, "old").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o750)).unwrap();
    }

    let write = |path: &str| {
        edit(
            &r,
            &FileAction::Write {
                path: rel(path),
                text: "new\r\n".into(),
            },
        )
    };
    write("run.sh").unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), "new\r\n");
    assert!(!t.path().join(".run.sh.beep-part").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }

    fs::create_dir(t.path().join("dir")).unwrap();
    assert!(write("dir").is_err());
    assert!(write("missing").is_err());
    assert!(write(".git/config").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&file, t.path().join("link")).unwrap();
        assert!(write("link").is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "new\r\n");
    }
}
