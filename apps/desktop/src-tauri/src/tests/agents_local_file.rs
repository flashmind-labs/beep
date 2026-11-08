use super::*;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::{env, fs, process};

fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("beep-local-file-{name}-{}", process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("root/src")).expect("scratch folder");
    fs::write(dir.join("root/src/a.txt"), "a").expect("file");
    fs::write(dir.join("outside.txt"), "o").expect("outside file");
    dir
}

#[test]
fn a_file_in_the_folder_resolves() {
    let dir = scratch("in");
    let got = inside(&dir.join("root"), "src/a.txt").expect("inside");
    assert!(got.ends_with("root/src/a.txt"));
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_file_outside_the_folder_is_refused() {
    let dir = scratch("out");
    let root = dir.join("root");
    assert!(inside(&root, "../outside.txt").is_none());
    assert!(inside(&root, "src/../../outside.txt").is_none());
    let absolute = dir.join("outside.txt");
    assert!(inside(&root, absolute.to_str().expect("utf8")).is_none());
    assert!(inside(&root, "src/missing.txt").is_none());
    fs::remove_dir_all(dir).expect("cleanup");
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_folder_is_refused() {
    let dir = scratch("link");
    let root = dir.join("root");
    symlink(dir.join("outside.txt"), root.join("src/link.txt")).expect("file link");
    symlink(&dir, root.join("up")).expect("folder link");
    assert!(inside(&root, "src/link.txt").is_none());
    assert!(inside(&root, "up/outside.txt").is_none());
    fs::remove_dir_all(dir).expect("cleanup");
}

#[cfg(unix)]
#[test]
fn a_link_inside_the_folder_resolves() {
    let dir = scratch("same");
    let root = dir.join("root");
    symlink(root.join("src/a.txt"), root.join("b.txt")).expect("link");
    assert!(inside(&root, "b.txt").is_some());
    fs::remove_dir_all(dir).expect("cleanup");
}
