use super::*;
use crate::agents::claude::hooks::{EVENTS, installed};
use serde_json::json;
use std::fs;

const COMMAND: &str = "'/opt/beep/beep-node' --dir '/srv/node' hook";

#[test]
fn stopping_removes_hooks_and_keeps_settings_edited_while_running() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    let installation = Installation::at(path.clone(), COMMAND.into()).unwrap();
    let mut settings = read_settings(&path).unwrap();
    assert_eq!(installed(&settings).len(), EVENTS.len());

    settings["model"] = json!("opus");
    settings["hooks"]["Stop"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type": "command", "command": "say done"}));
    write_settings(&path, &settings).unwrap();

    drop(installation);
    assert_eq!(
        read_settings(&path).unwrap(),
        json!({
            "model": "opus",
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}
        })
    );
}

#[test]
fn stopping_an_old_node_keeps_a_new_nodes_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    let old = Installation::at(path.clone(), COMMAND.into()).unwrap();
    let command = "'/opt/beep/beep-node' --dir '/srv/other-node' hook";
    let new = Installation::at(path.clone(), command.into()).unwrap();
    let before = fs::read(&path).unwrap();

    drop(old);
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(new);
    assert_eq!(read_settings(&path).unwrap(), json!({}));
}

#[test]
fn hooks_left_by_a_previous_run_are_removed_on_exit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    install(&path, COMMAND).unwrap();
    let installation = Installation::at(path.clone(), COMMAND.into()).unwrap();
    drop(installation);
    assert_eq!(read_settings(&path).unwrap(), json!({}));
}

#[test]
fn invalid_settings_are_left_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    fs::write(&path, "{unfinished").unwrap();
    assert!(Installation::at(path.clone(), COMMAND.into()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{unfinished");

    fs::remove_file(&path).unwrap();
    let installation = Installation::at(path.clone(), COMMAND.into()).unwrap();
    fs::write(&path, "{edited").unwrap();
    drop(installation);
    assert_eq!(fs::read_to_string(&path).unwrap(), "{edited");
}
