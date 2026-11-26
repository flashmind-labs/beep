use super::*;
use crate::fs::paths;
use std::io::Write;

/// A temporary folder and its real path, which the checks compare against.
fn temp_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("made");
    let real = paths::canonicalize(dir.path()).expect("exists");
    (dir, real)
}

fn target(dir: &Path, folder: &str) -> Arc<Folder> {
    let folders = Folders::new(&[dir.to_owned()]);
    Arc::new(open_target(&folders, dir.join(folder).to_str().unwrap()).unwrap())
}

/// Writes `data` as `name` in a new partial folder of `target`.
fn written(journal: &Journal, target: &Arc<Folder>, name: &str, data: &str) -> Partial {
    let partial = journal.start(target.clone()).unwrap();
    let mut file = partial.dir().create_file(OsStr::new(name), false).unwrap();
    file.write_all(data.as_bytes()).unwrap();
    partial
}

#[test]
fn names_from_the_source_are_plain() {
    assert_eq!(plain_name("render.png").unwrap(), "render.png");

    for bad in ["", ".", "..", "a/b", "/abs", "a/", "../x"] {
        assert!(plain_name(bad).is_err(), "{bad:?}");
    }

    assert_eq!(
        relative_path("frames/0001.png").unwrap(),
        PathBuf::from("frames/0001.png")
    );

    for bad in ["", "/etc/passwd", "../out", "a/../../b", "a/./b", "a//b"] {
        assert!(relative_path(bad).is_err(), "{bad:?}");
    }
}

#[cfg(unix)]
#[test]
fn the_target_folder_is_absolute_existing_and_shared() {
    let (_temp, dir) = temp_dir();
    let shared = dir.join("shared");
    fs::create_dir_all(shared.join("in/deep")).expect("made");
    fs::write(shared.join("file.txt"), "x").expect("written");
    let outside = dir.join("outside");
    fs::create_dir_all(&outside).expect("made");
    std::os::unix::fs::symlink(shared.join("in"), shared.join("link")).expect("linked");
    let folders = Folders::new(std::slice::from_ref(&shared));

    let found = open_target(&folders, shared.join("in/deep").to_str().unwrap()).unwrap();
    assert_eq!(found.path(), shared.join("in/deep"));
    let relative = open_target(&folders, "in").unwrap_err();
    assert!(format!("{relative:#}").contains("not an absolute path"));
    let missing = open_target(&folders, shared.join("nope").to_str().unwrap()).unwrap_err();
    assert!(
        format!("{missing:#}").contains("does not exist"),
        "{missing:#}"
    );
    assert!(open_target(&folders, shared.join("file.txt").to_str().unwrap()).is_err());
    let out = open_target(&folders, outside.to_str().unwrap()).unwrap_err();
    assert!(format!("{out:#}").contains("outside the shared folders"));
    // A link inside the shared folder resolves to where it points.
    let linked = open_target(&folders, shared.join("link").to_str().unwrap()).unwrap();
    assert_eq!(linked.path(), shared.join("in"));
}

#[test]
fn placing_keeps_both_and_never_replaces() {
    let (_temp, dir) = temp_dir();
    let folder = dir.join("in");
    fs::create_dir_all(&folder).expect("made");
    fs::write(folder.join("render.png"), "old").expect("written");
    fs::write(folder.join("render (2).png"), "older").expect("written");
    let journal = Journal::open(dir.join("journal")).unwrap();
    let target = target(&dir, "in");

    let partial = written(&journal, &target, "render.png", "new");
    let placed = place(&partial, "render.png", ContentKind::File).unwrap();
    partial.finish().unwrap();
    assert_eq!(placed, folder.join("render (3).png"));
    assert_eq!(
        fs::read_to_string(folder.join("render.png")).unwrap(),
        "old"
    );
    assert_eq!(fs::read_to_string(&placed).unwrap(), "new");

    // A folder never lands on a folder, empty or not, or on a file.
    fs::create_dir_all(folder.join("frames")).expect("made");
    fs::write(folder.join("frames (2)"), "a file").expect("written");
    let partial = journal.start(target.clone()).unwrap();
    let frames = make_dir_at(partial.dir(), OsStr::new("frames")).unwrap();
    frames.create_file(OsStr::new("1.png"), false).unwrap();
    let placed = place(&partial, "frames", ContentKind::Folder).unwrap();
    partial.finish().unwrap();
    assert_eq!(placed, folder.join("frames (3)"));
    assert!(placed.join("1.png").is_file());
    assert_eq!(fs::read_dir(dir.join("journal")).unwrap().count(), 0);
}

#[test]
fn a_partial_folder_fits_a_long_name_and_reads_as_partial() {
    let (_temp, dir) = temp_dir();
    fs::create_dir_all(dir.join("in")).expect("made");
    let journal = Journal::open(dir.join("journal")).unwrap();
    let target = target(&dir, "in");
    let long = format!("{}.bin", "r".repeat(250));
    let partial = written(&journal, &target, &long, "x");
    let hidden = entries(&dir.join("in"));
    assert!(
        hidden[0].starts_with(".beep-") && hidden[0].ends_with(".partial"),
        "{hidden:?}"
    );
    let placed = place(&partial, &long, ContentKind::File).unwrap();
    partial.finish().unwrap();
    assert_eq!(placed, dir.join("in").join(&long));
    assert_eq!(entries(&dir.join("in")), [long]);
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_stopped_nodes_partial_folders_are_swept() {
    let (_temp, dir) = temp_dir();
    let folder = dir.join("in");
    fs::create_dir_all(&folder).expect("made");
    let journal = Journal::open(dir.join("journal")).unwrap();
    let target = target(&dir, "in");
    let partial = written(&journal, &target, "big.bin", "half");
    // As if the node was killed: nothing removes it now.
    std::mem::forget(partial);
    assert_eq!(entries(&folder).len(), 1);
    // An entry that cannot be read is skipped, not fatal.
    fs::create_dir(dir.join("journal/unreadable")).expect("made");
    journal.sweep().unwrap();
    assert!(entries(&folder).is_empty());
    assert_eq!(entries(&dir.join("journal")), ["unreadable"]);

    let partial = journal.start(target.clone()).unwrap();
    let deep = make_dirs_at(partial.dir(), Path::new("a/b/c")).unwrap();
    deep.create_file(OsStr::new("x"), false).unwrap();
    partial.remove().unwrap();
    assert!(entries(&folder).is_empty());
}

#[test]
fn room_counts_one_disk_twice() {
    let disk = |free, device| Space { free, device };
    let fits = |need, store, folder| {
        Arc::new(Room::default())
            .reserve(need, store, folder, 10)
            .is_ok()
    };
    assert!(fits(40, disk(100, 1), disk(100, 1)));
    assert!(!fits(50, disk(100, 1), disk(100, 1)));
    assert!(fits(50, disk(100, 1), disk(100, 2)));
    assert!(!fits(95, disk(100, 1), disk(200, 2)));
    assert!(!fits(95, disk(200, 1), disk(100, 2)));
    assert!(!fits(u64::MAX, disk(100, 1), disk(100, 1)));
}
