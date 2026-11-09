use super::*;
use std::os::unix::fs::symlink;
use tempfile::TempDir;

#[test]
fn changed_paths_do_not_follow_links_or_visit_unrelated_subtrees() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("private.txt"), "private").unwrap();
    fs::write(dir.path().join("one.txt"), "one").unwrap();
    fs::create_dir(dir.path().join("unrelated")).unwrap();
    fs::write(dir.path().join("unrelated/two.txt"), "two").unwrap();
    symlink(outside.path(), dir.path().join("link")).unwrap();
    let rules = Rules::load(dir.path()).unwrap();
    let selected = [PathBuf::from("one.txt"), PathBuf::from("link/private.txt")].into();
    let walked = walk_changed(dir.path(), &rules, &selected, false, false).unwrap();
    assert_eq!(
        walked
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["one.txt"]
    );
    assert!(walked.dirs.is_empty());
}

#[test]
fn a_walk_keeps_files_lists_folders_and_says_what_it_left_out() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    for file in [
        "Cargo.toml",
        "src/main.rs",
        "target/debug/app",
        "assets/a.png",
    ] {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, file).unwrap();
    }
    fs::create_dir(root.join(".beep-1.partial")).unwrap();
    symlink("/etc/hostname", root.join("link")).unwrap();
    let rules = Rules::load(root).unwrap();
    let walked = walk(root, &rules, 10).unwrap();
    let files: Vec<_> = walked.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(files, ["Cargo.toml", "assets/a.png", "src/main.rs"]);
    let mut dirs: Vec<_> = walked
        .dirs
        .iter()
        .map(|d| d.strip_prefix(root).unwrap().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    assert_eq!(dirs, ["", "assets", "src"]);
    let mut skipped: Vec<_> = walked
        .skipped
        .iter()
        .map(|s| (s.path.as_str(), <&str>::from(s.rule)))
        .collect();
    skipped.sort();
    assert_eq!(skipped, [("link", "link"), ("target", "default")]);
    assert_eq!(walked.files[0].stat.size, "Cargo.toml".len() as u64);

    let few = walk(root, &rules, 1).unwrap();
    assert_eq!(few.skipped.len(), 1);
    assert!(few.skipped_more);
}
