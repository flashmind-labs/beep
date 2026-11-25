use super::*;
use std::os::unix::fs::symlink;

#[test]
fn a_folder_walk_keeps_files_and_skips_links() {
    let dir = std::env::temp_dir().join(format!("beep-node-walk-{}", std::process::id()));
    // A leftover from an earlier run with the same process id.
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("frames/deep")).expect("made");
    fs::create_dir_all(dir.join("empty")).expect("made");
    fs::write(dir.join("scene.blend"), "abc").expect("written");
    fs::write(dir.join("frames/0001.png"), "1").expect("written");
    fs::write(dir.join("frames/deep/x.txt"), "xy").expect("written");
    fs::create_dir_all(dir.join("links-only")).expect("made");
    symlink("/etc/hostname", dir.join("link")).expect("linked");
    symlink("/etc/hostname", dir.join("links-only/link")).expect("linked");
    fsat::set_mode(&dir.join("scene.blend"), 0o755).expect("made executable");
    let walked = walk(&dir).unwrap();
    let names: Vec<_> = walked.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        ["frames/0001.png", "frames/deep/x.txt", "scene.blend"]
    );
    assert_eq!(walked.skipped, 2);
    assert_eq!(walked.entries[1].size, 2);
    assert!(walked.entries[2].executable);
    assert!(!walked.entries[0].executable);
    assert_eq!(walked.empty_folders, ["empty", "links-only"]);
    fs::remove_dir_all(&dir).expect("removed");
}

#[test]
fn a_file_swapped_after_the_walk_is_refused() {
    let dir = std::env::temp_dir().join(format!("beep-node-swap-{}", std::process::id()));
    // A leftover from an earlier run with the same process id.
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("made");
    let file = dir.join("render.png");
    fs::write(&file, "a").expect("written");
    fs::write(dir.join("other.png"), "b").expect("written");
    let walked = fsat::id_of_metadata(&fs::metadata(&file).expect("exists"));
    fsat::check_same(&file, walked).unwrap();

    fs::remove_file(&file).expect("removed");
    symlink(dir.join("other.png"), &file).expect("linked");
    assert!(fsat::check_same(&file, walked).is_err());

    fs::remove_file(&file).expect("removed");
    fs::rename(dir.join("other.png"), &file).expect("moved");
    let changed = fsat::check_same(&file, walked).unwrap_err();
    assert!(format!("{changed:#}").contains("changed while it was being sent"));
    fs::remove_dir_all(&dir).expect("removed");
}
