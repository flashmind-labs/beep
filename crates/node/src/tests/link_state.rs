use super::*;
use crate::fs::paths;

/// A machine that joins a mesh before it ever ran has no folder: starting
/// its state makes the folder and key a service launch needs.
#[test]
fn a_new_node_gets_its_folder_and_key() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("not").join("yet");
    assert!(!dir.exists());
    let state = State::load_or_start(&dir, "box").unwrap();
    assert!(paths::canonicalize(&dir).is_ok());
    let again = State::load_or_start(&dir, "other name").unwrap();
    assert_eq!(again.name, "box");
    assert_eq!(again.key.public(), state.key.public());
}

#[test]
fn settings_without_a_key_are_never_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    State::load_or_start(dir, "box").unwrap();
    let settings = fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
    fs::remove_file(dir.join(KEY_FILE)).unwrap();
    assert!(State::load_or_start(dir, "other").is_err());
    assert_eq!(
        fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap(),
        settings
    );
    assert!(!dir.join(KEY_FILE).exists());
}

/// A node.toml from a node that was linked to a server, or that still
/// has `webhook_listen` (#339), loads; saving drops what is gone.
#[test]
fn old_settings_load_and_saving_drops_them() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    State::load_or_start(dir, "box").unwrap();
    let path = dir.join(SETTINGS_FILE);
    let old = fs::read_to_string(&path).unwrap()
        + "server = \"abc\"\nwebhook_listen = \"[fd00::12]:7480\"\n";
    fs::write(&path, old).unwrap();
    assert_eq!(State::load(dir).unwrap().name, "box");
    save_search(dir, &Search::from("~")).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("webhook_listen"), "{saved}");
    assert!(!saved.contains("server"), "{saved}");
}

#[test]
fn a_folder_without_settings_searches_home() {
    let dir = tempfile::tempdir().expect("a folder");
    assert_eq!(saved_search(dir.path()).expect("home"), Search::default());
}
