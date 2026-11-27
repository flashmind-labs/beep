use super::*;
use crate::agents::checkout::tests::{has_git, run as run_git};
use tempfile::TempDir;

fn root_in(dir: &TempDir) -> WorkRoot {
    WorkRoot::at(&paths::canonicalize(dir.path()).expect("a temp folder resolves"))
        .expect("a temp folder is a root")
}

fn names_of(folder: &WorkFolder) -> Vec<&str> {
    folder.entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn paths_stay_inside_the_root() {
    assert_eq!(names("src/./lib.rs").unwrap(), PathBuf::from("src/lib.rs"));
    assert_eq!(names("").unwrap(), PathBuf::new());

    for bad in ["../secret", "src/../../etc", "a/.."] {
        let e = names(bad).unwrap_err();
        assert!(e.to_string().contains("outside"), "{bad}: {e:#}");
    }
    // A leading slash is only an empty name; the path stays relative.
    assert_eq!(names("/etc/passwd").unwrap(), PathBuf::from("etc/passwd"));
}

#[test]
fn leaves_out_what_git_ignores_unless_asked() {
    let dir = TempDir::new().unwrap();
    // The checkout's `.git` is what makes `.gitignore` count.
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
    fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("build.log"), "").unwrap();
    fs::write(dir.path().join("src/run.log"), "").unwrap();
    fs::write(dir.path().join("src/main.rs"), "").unwrap();
    let root = root_in(&dir);
    assert_eq!(
        names_of(&list(&root, "", false, None).unwrap()),
        ["src", ".gitignore"]
    );
    let all = list(&root, "", true, None).unwrap();
    assert_eq!(names_of(&all), ["src", "target", ".gitignore", "build.log"]);
    let ignored: Vec<_> = all
        .entries
        .iter()
        .filter(|e| e.ignored)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(ignored, ["target", "build.log"]);
    // The root's `.gitignore` counts in a folder under it too.
    assert_eq!(
        names_of(&list(&root, "src", false, None).unwrap()),
        ["main.rs"]
    );
}

#[test]
fn marks_what_git_sees_changed() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let dir = TempDir::new().unwrap();
    let at = dir.path();
    run_git(at, &["init", "-q"]);
    fs::create_dir_all(at.join("src/deep")).unwrap();
    fs::write(at.join("src/deep/lib.rs"), "one").unwrap();
    fs::write(at.join("README.md"), "one").unwrap();
    run_git(at, &["add", "."]);
    run_git(at, &["commit", "-qm", "first"]);
    fs::write(at.join("src/deep/lib.rs"), "two").unwrap();
    fs::write(at.join("new.txt"), "").unwrap();
    fs::create_dir_all(at.join("notes")).unwrap();
    fs::write(at.join("notes/a.md"), "").unwrap();
    let root = root_in(&dir);
    let top = list(&root, "", false, None).unwrap();
    assert!(top.git);
    let change =
        |f: &WorkFolder, name: &str| f.entries.iter().find(|e| e.name == name).unwrap().change;
    assert_eq!(change(&top, "src"), Some(GitChange::Modified {}));
    assert_eq!(change(&top, "notes"), Some(GitChange::Untracked {}));
    assert_eq!(change(&top, "new.txt"), Some(GitChange::Untracked {}));
    assert_eq!(change(&top, "README.md"), None);
    let src = list(&root, "src", false, None).unwrap();
    assert_eq!(change(&src, "deep"), Some(GitChange::Modified {}));
}

fn paths_of(tree: &WorkTree) -> Vec<&str> {
    tree.folders.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn a_tree_reads_levels_down_to_its_depth() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    fs::create_dir_all(dir.path().join("src/deep/deeper")).unwrap();
    fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    fs::write(dir.path().join("src/deep/lib.rs"), "").unwrap();
    let root = root_in(&dir);
    let two = tree(&root, "", false, 2, None).unwrap();
    assert_eq!(paths_of(&two), ["", "src"]);
    assert!(!two.truncated);
    let three = tree(&root, "", false, 3, None).unwrap();
    assert_eq!(paths_of(&three), ["", "src", "src/deep"]);
    assert_eq!(names_of(&three.folders[2]), ["deeper", "lib.rs"]);
    // From a folder inside, paths stay relative to the root.
    assert_eq!(
        paths_of(&tree(&root, "src", false, 2, None).unwrap()),
        ["src", "src/deep"]
    );
    // What git ignores is listed when asked, but not read into.
    let all = tree(&root, "", true, 3, None).unwrap();
    assert_eq!(names_of(&all.folders[0]), ["src", "target", ".gitignore"]);
    assert_eq!(paths_of(&all), ["", "src", "src/deep"]);
}

#[test]
fn a_tree_stops_past_its_entries() {
    let dir = TempDir::new().unwrap();

    for folder in ["a", "b", "c"] {
        let at = dir.path().join(folder);
        fs::create_dir_all(&at).unwrap();

        for n in 0..MAX_WORK_ENTRIES {
            fs::write(at.join(n.to_string()), "").unwrap();
        }
    }

    let got = tree(&root_in(&dir), "", false, 3, None).unwrap();
    assert_eq!(paths_of(&got), ["", "a", "b"]);
    assert!(got.truncated);
}

#[test]
fn a_tree_marks_changes_in_each_folder() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let dir = TempDir::new().unwrap();
    let at = dir.path();
    run_git(at, &["init", "-q"]);
    fs::create_dir_all(at.join("src/deep")).unwrap();
    fs::write(at.join("src/deep/lib.rs"), "one").unwrap();
    fs::write(at.join("src/main.rs"), "one").unwrap();
    run_git(at, &["add", "."]);
    run_git(at, &["commit", "-qm", "first"]);
    fs::write(at.join("src/deep/lib.rs"), "two").unwrap();
    let got = tree(&root_in(&dir), "src", false, 2, None).unwrap();
    let change =
        |f: &WorkFolder, name: &str| f.entries.iter().find(|e| e.name == name).unwrap().change;
    assert_eq!(
        change(&got.folders[0], "deep"),
        Some(GitChange::Modified {})
    );
    assert_eq!(change(&got.folders[0], "main.rs"), None);
    assert_eq!(
        change(&got.folders[1], "lib.rs"),
        Some(GitChange::Modified {})
    );
    assert!(got.folders[1].git);
}

#[test]
fn reads_status_by_the_first_name_under_the_folder() {
    let status =
        "M  src/a.rs\0?? src/new/\0A  src/b.rs\0UU src/c.rs\0R  src/d.rs\0old/d.rs\0 M top.rs\0";
    let got = changes_in(status, "src/");
    assert_eq!(got.get("a.rs"), Some(&GitChange::Modified {}));
    assert_eq!(got.get("new"), Some(&GitChange::Untracked {}));
    assert_eq!(got.get("b.rs"), Some(&GitChange::Added {}));
    assert_eq!(got.get("c.rs"), Some(&GitChange::Conflicted {}));
    assert_eq!(got.get("d.rs"), Some(&GitChange::Modified {}));
    assert_eq!(got.len(), 5, "{got:?}");
    let top = changes_in(status, "");
    assert_eq!(top.get("src"), Some(&GitChange::Conflicted {}));
    assert_eq!(top.get("top.rs"), Some(&GitChange::Modified {}));
    assert_eq!(top.get("old"), None);
}

#[test]
fn a_local_copy_reads_by_path_and_a_file_not_there_is_none() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("models")).unwrap();
    fs::write(dir.path().join("models/robot.glb"), b"glTF").unwrap();

    let part = local_read(dir.path(), "models/robot.glb", 0, 16)
        .unwrap()
        .expect("the file is there");
    assert_eq!(part.size, 4);
    assert_eq!(part.mime, "model/gltf-binary");
    assert_eq!(part.data, b"glTF");
    assert!(
        local_read(dir.path(), "models/late.glb", 0, 16)
            .unwrap()
            .is_none()
    );
    let tree = local_tree(dir.path(), "", false, 2).unwrap();
    assert_eq!(tree.folders[1].path, "models");
    assert!(local_tree(&dir.path().join("gone"), "", false, 2).is_err());
}

#[test]
fn reads_any_file_a_part_at_a_time() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("notes.txt"), "0123456789").unwrap();
    let root = root_in(&dir);
    let chunk = read(&root, "notes.txt", 2, 3).unwrap().unwrap();
    assert_eq!(chunk.size, 10);
    assert_eq!(chunk.data, b"234");
    assert_eq!(chunk.mime, "application/octet-stream");
    assert!(read(&root, "gone.txt", 0, 10).unwrap().is_none());
    assert!(read(&root, "../notes.txt", 0, 10).is_err());
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_root_is_refused() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        dir.path().join("link.txt"),
    )
    .unwrap();
    let root = root_in(&dir);
    for e in [
        list(&root, "out", true, None).unwrap_err(),
        read(&root, "out/secret.txt", 0, 10).unwrap_err(),
        read(&root, "link.txt", 0, 10).unwrap_err(),
        write(&root, "out/dropped.txt", 0, 2, b"hi").unwrap_err(),
    ] {
        assert!(e.to_string().contains("outside"), "{e:#}");
    }
    assert!(!outside.path().join("dropped.txt").exists());
}

#[test]
fn uploads_land_in_beep_uploads_under_a_free_name() {
    let dir = TempDir::new().unwrap();
    let root = root_in(&dir);
    let first = write(&root, ".beep/uploads/build.log", 0, 3, b"one").unwrap();
    assert_eq!(first, ".beep/uploads/build.log");
    let second = write(&root, ".beep/uploads/build.log", 0, 3, b"two").unwrap();
    assert_eq!(second, ".beep/uploads/build 2.log");
    assert_eq!(
        fs::read(dir.path().join(".beep/uploads/build 2.log")).unwrap(),
        b"two"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".beep/.gitignore")).unwrap(),
        "*\n"
    );
}

#[test]
fn a_file_in_parts_shows_once_its_last_part_is_in() {
    let dir = TempDir::new().unwrap();
    let root = root_in(&dir);
    let at = write(&root, "src/data.bin", 0, 6, b"abc").unwrap();
    assert_eq!(at, "src/data.bin");
    assert!(!dir.path().join("src/data.bin").exists());
    // A part out of order is refused, and the next one goes on.
    assert!(write(&root, &at, 4, 6, b"ef").is_err());
    assert_eq!(write(&root, &at, 3, 6, b"def").unwrap(), at);
    assert_eq!(
        fs::read(dir.path().join("src/data.bin")).unwrap(),
        b"abcdef"
    );
    assert!(!dir.path().join("src/.data.bin.beep-part").exists());
    // No `.gitignore` outside `.beep`.
    assert!(!dir.path().join("src/.gitignore").exists());
}

#[test]
fn refuses_invalid_parts_and_paths() {
    let dir = TempDir::new().unwrap();
    let root = root_in(&dir);
    assert!(
        write(
            &root,
            "big.bin",
            0,
            u64::MAX,
            &vec![0; AGENT_FILE_CHUNK + 1]
        )
        .is_err()
    );
    assert!(write(&root, "short.bin", 0, 1, b"xy").is_err());
    let out = write(&root, "../escape.txt", 0, 1, b"x").unwrap_err();
    assert!(out.to_string().contains("outside"), "{out:#}");
    assert!(write(&root, "", 0, 1, b"x").is_err());
}

#[test]
fn large_project_uploads_complete_on_disk_without_a_size_cap() {
    let dir = TempDir::new().unwrap();
    let root = root_in(&dir);
    let size = 6 * 1024 * 1024 * 1024;
    let path = write(&root, "large.mov", 0, size, b"a").unwrap();
    OpenOptions::new()
        .write(true)
        .open(dir.path().join(".large.mov.beep-part"))
        .unwrap()
        .set_len(size - 1)
        .unwrap();
    assert_eq!(write(&root, &path, size - 1, size, b"z").unwrap(), path);
    let mut file = fs::File::open(dir.path().join(path)).unwrap();
    assert_eq!(file.metadata().unwrap().len(), size);
    file.seek(SeekFrom::End(-1)).unwrap();
    let mut last = [0];
    file.read_exact(&mut last).unwrap();
    assert_eq!(last, *b"z");
}

#[test]
fn a_hash_changes_with_the_content_and_an_unchanged_file_is_not_read_again() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("site")).unwrap();
    let page = dir.path().join("site/index.html");
    fs::write(&page, "color: #fff").unwrap();
    let root = root_in(&dir);
    let ask = |path: &str| hashes(&root, &[path.to_owned()])[0];
    let first = ask("site/index.html").expect("a file has a hash");

    // Same size, new content, a later time: a new hash.
    fs::write(&page, "color: #000").unwrap();
    let file = fs::File::options().write(true).open(&page).unwrap();
    let later = SystemTime::now() + std::time::Duration::from_secs(5);
    file.set_modified(later).unwrap();
    let second = ask("site/index.html").expect("a file has a hash");
    assert_ne!(first, second);

    // The same size and time again are taken as unchanged: the kept hash
    // comes back, though the bytes differ, as the file is not read.
    fs::write(&page, "color: #abc").unwrap();
    file.set_modified(later).unwrap();
    assert_eq!(ask("site/index.html"), Some(second));

    assert_eq!(ask("site/gone.html"), None);
    assert_eq!(ask("site"), None);
    assert_eq!(ask("../outside"), None);
}

/// The folders under `at` as paths inside it, `at` itself as `""`.
fn folders_under(at: &Path, rel: &str, out: &mut Vec<String>) {
    out.push(rel.to_owned());

    for item in fs::read_dir(at).unwrap().flatten() {
        let name = item.file_name().to_string_lossy().into_owned();

        if name != ".git" && item.file_type().unwrap().is_dir() {
            folders_under(&item.path(), &joined(rel, &name), out);
        }
    }
}

/// Lists and reads trees of every folder in `root` from the status of the
/// whole root, and again with git asked in each folder; the two agree.
fn assert_kept_status_agrees(root: &WorkRoot, trees: bool) {
    let kept = git_status(&root.root).expect("a checkout");
    let mut all = Vec::new();
    folders_under(&root.root, "", &mut all);
    assert!(all.len() > 1);

    for path in all {
        for ignored in [false, true] {
            assert_eq!(
                list(root, &path, ignored, Some(&kept)).unwrap(),
                list(root, &path, ignored, None).unwrap(),
                "listing {path:?}"
            );
            if trees {
                assert_eq!(
                    tree(root, &path, ignored, 3, Some(&kept)).unwrap(),
                    tree(root, &path, ignored, 3, None).unwrap(),
                    "tree of {path:?}"
                );
            }
        }
    }
}

fn repo_with_changes(dir: &TempDir) {
    let at = dir.path();
    run_git(at, &["init", "-q"]);
    fs::write(at.join(".gitignore"), "build/\n*.log\n").unwrap();
    fs::create_dir_all(at.join("src/deep")).unwrap();
    fs::create_dir_all(at.join("old")).unwrap();
    fs::write(at.join("src/deep/lib.rs"), "one").unwrap();
    fs::write(at.join("src/keep.rs"), "one").unwrap();
    fs::write(
        at.join("old/moved.txt"),
        "line one\nline two\nline three\nline four\n",
    )
    .unwrap();
    fs::write(
        at.join("old/inner.txt"),
        "line one\nline two\nline three\nline four\n",
    )
    .unwrap();
    fs::write(at.join("old/staying.txt"), "x").unwrap();
    run_git(at, &["add", "."]);
    run_git(at, &["commit", "-qm", "first"]);
    fs::write(at.join("src/deep/lib.rs"), "two").unwrap();
    // A folder git collapses into one entry, with a folder inside it.
    fs::create_dir_all(at.join("fresh/inner")).unwrap();
    fs::write(at.join("fresh/inner/a.txt"), "a").unwrap();
    fs::write(at.join("fresh/b.txt"), "b").unwrap();
    // A tracked folder with an untracked one in it, and ignored files.
    fs::create_dir_all(at.join("src/new")).unwrap();
    fs::write(at.join("src/new/c.txt"), "c").unwrap();
    fs::create_dir_all(at.join("build")).unwrap();
    fs::write(at.join("build/out.o"), "o").unwrap();
    fs::write(at.join("src/debug.log"), "l").unwrap();
    // A rename across folders, both ways, and one within a folder.
    run_git(at, &["mv", "old/moved.txt", "src/moved.txt"]);
    run_git(at, &["mv", "old/inner.txt", "old/renamed.txt"]);
    // A staged new file and a deleted one.
    fs::write(at.join("src/staged.rs"), "s").unwrap();
    run_git(at, &["add", "src/staged.rs"]);
    fs::remove_file(at.join("old/staying.txt")).unwrap();
}

#[test]
fn a_status_at_the_root_marks_each_folder_as_git_in_it_does() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let dir = TempDir::new().unwrap();
    repo_with_changes(&dir);
    assert_kept_status_agrees(&root_in(&dir), true);

    // A folder git collapses lists no mark for what is in it, as before.
    let kept = git_status(&root_in(&dir).root).unwrap();
    let inside = list(&root_in(&dir), "fresh", false, Some(&kept)).unwrap();
    assert!(inside.git && inside.entries.iter().all(|e| e.change.is_none()));
    let top = list(&root_in(&dir), "", false, Some(&kept)).unwrap();
    let change = |name: &str| top.entries.iter().find(|e| e.name == name).unwrap().change;
    assert_eq!(change("fresh"), Some(GitChange::Untracked {}));
    assert_eq!(change("src"), Some(GitChange::Modified {}));
}

#[test]
fn a_root_inside_a_repository_reads_the_status_from_its_prefix() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let dir = TempDir::new().unwrap();
    repo_with_changes(&dir);
    let root = WorkRoot::at(&paths::canonicalize(dir.path().join("src")).unwrap()).unwrap();
    assert_kept_status_agrees(&root, true);
    let kept = git_status(&root.root).unwrap();
    let top = list(&root, "", false, Some(&kept)).unwrap();
    let change = |name: &str| top.entries.iter().find(|e| e.name == name).unwrap().change;
    assert_eq!(change("moved.txt"), Some(GitChange::Added {}));
    assert_eq!(change("new"), Some(GitChange::Untracked {}));
}

#[test]
fn a_nested_repository_is_read_in_itself() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let dir = TempDir::new().unwrap();
    repo_with_changes(&dir);
    let nested = dir.path().join("vendor/lib");
    fs::create_dir_all(nested.join("pkg")).unwrap();
    run_git(&nested, &["init", "-q"]);
    fs::write(nested.join("pkg/x.txt"), "x").unwrap();
    run_git(&nested, &["add", "."]);
    run_git(&nested, &["commit", "-qm", "first"]);
    fs::write(nested.join("pkg/x.txt"), "y").unwrap();
    fs::write(nested.join("z.txt"), "z").unwrap();
    let root = root_in(&dir);
    assert_kept_status_agrees(&root, true);
    let kept = git_status(&root.root).unwrap();
    let lib = list(&root, "vendor/lib", false, Some(&kept)).unwrap();
    let change = |name: &str| lib.entries.iter().find(|e| e.name == name).unwrap().change;
    assert_eq!(change("pkg"), Some(GitChange::Modified {}));
    assert_eq!(change("z.txt"), Some(GitChange::Untracked {}));
}

#[test]
fn the_kept_status_serves_a_folder_unless_the_watch_skips_it() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let dir = TempDir::new().unwrap();
    repo_with_changes(&dir);
    fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    fs::create_dir_all(dir.path().join("src/node_modules")).unwrap();
    let root = root_in(&dir);
    let kept = git_status(&root.root).unwrap();
    let from = |path: &str| status_in(&root, &root.root.join(path), Some(&kept)).unwrap();
    assert!(Arc::ptr_eq(&from("").out, &kept.out));
    assert!(Arc::ptr_eq(&from("src/deep").out, &kept.out));
    assert_eq!(from("src/deep").prefix, "src/deep/");
    // A change under these is not heard, so git is asked.
    assert!(!Arc::ptr_eq(&from("target/debug").out, &kept.out));
    assert!(!Arc::ptr_eq(&from("src/node_modules").out, &kept.out));
    // So is a folder a rename crosses the edge of.
    assert!(!Arc::ptr_eq(&from("src").out, &kept.out));
}

#[test]
fn a_tree_reads_a_tracked_folder_the_watch_skips_in_itself() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let dir = TempDir::new().unwrap();
    let at = dir.path();
    run_git(at, &["init", "-q"]);
    fs::create_dir_all(at.join("target/debug")).unwrap();
    fs::write(at.join("target/debug/out.txt"), "one").unwrap();
    run_git(at, &["add", "-f", "."]);
    run_git(at, &["commit", "-qm", "first"]);
    fs::write(at.join("target/debug/out.txt"), "two").unwrap();
    let root = root_in(&dir);
    let kept = git_status(&root.root).unwrap();
    let got = tree(&root, "", true, 3, Some(&kept)).unwrap();
    assert_eq!(got, tree(&root, "", true, 3, None).unwrap());
    let debug = got
        .folders
        .iter()
        .find(|f| f.path == "target/debug")
        .unwrap();
    assert_eq!(debug.entries[0].change, Some(GitChange::Modified {}));
}
