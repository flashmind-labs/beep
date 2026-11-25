use super::*;
use crate::files::paths;
use crate::{files::Folders, files::place::open_target};
use tempfile::TempDir;

fn open(dir: &TempDir, name: &str) -> Folder {
    let folders = Folders::new(&[dir.path().to_owned()]);
    let path = paths::canonicalize(dir.path()).unwrap().join(name);
    open_target(&folders, &path.to_string_lossy()).unwrap()
}

#[test]
fn a_folder_keeps_its_id_and_a_swapped_one_is_refused() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("game")).unwrap();
    let root = open(&dir, "game");
    let Checked::Same(first) = check(&root, None, false).unwrap() else {
        panic!("a new folder is marked");
    };
    assert!(dir.path().join("game").join(MARKER).exists());
    let same = Checked::Same(first.clone());
    assert_eq!(check(&root, Some(&first), true).unwrap(), same);

    // The marker removed: not trusted, nor written, until restored.
    fs::remove_file(dir.path().join("game").join(MARKER)).unwrap();
    assert_eq!(
        check(&root, Some(&first), true).unwrap(),
        Checked::Unmarked(first.clone())
    );
    assert!(!dir.path().join("game").join(MARKER).exists());
    restore(&root, &first).unwrap();
    assert_eq!(check(&root, Some(&first), true).unwrap(), same);

    // Swapped for an empty folder at the same path.
    fs::rename(dir.path().join("game"), dir.path().join("old")).unwrap();
    fs::create_dir(dir.path().join("game")).unwrap();
    let error = check(&root, Some(&first), true).unwrap_err();
    assert!(format!("{error:#}").contains("not mounted"), "{error:#}");
    let reopened = open(&dir, "game");
    let error = check(&reopened, Some(&first), true).unwrap_err();
    assert!(format!("{error:#}").contains("not mounted"), "{error:#}");
    assert!(!dir.path().join("game").join(MARKER).exists());
    // With nothing seen there yet, it simply starts.
    assert!(check(&reopened, Some(&first), false).is_ok());
}

#[test]
fn the_marker_stays_out_of_git_status() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("repo/.git/info")).unwrap();
    let root = open(&dir, "repo");
    check(&root, None, false).unwrap();
    let Checked::Same(first) = check(&root, None, false).unwrap() else {
        panic!("a new folder is marked");
    };
    fs::remove_file(dir.path().join("repo").join(MARKER)).unwrap();
    restore(&root, &first).unwrap();
    let exclude = fs::read_to_string(dir.path().join("repo/.git/info/exclude")).unwrap();
    assert_eq!(exclude, format!("/{MARKER}\n"));
}
