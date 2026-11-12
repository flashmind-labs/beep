use super::*;

#[test]
fn only_a_folder_that_is_there_opens() {
    let dir = std::env::temp_dir();
    assert!(openable(&dir));
    assert!(!openable(Path::new("relative/folder")));
    assert!(!openable(&dir.join("no-such-folder-for-beep")));
}
