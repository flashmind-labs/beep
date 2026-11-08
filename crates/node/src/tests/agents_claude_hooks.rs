use super::*;

const COMMAND: &str = "'/opt/beep/beep-node' --dir '/srv/node' hook";

#[test]
fn install_adds_each_event_once_and_keeps_other_hooks() {
    let mut settings = json!({
        "model": "opus",
        "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
        }
    });
    let names: Vec<&str> = EVENTS.iter().map(|&(event, _)| event).collect();
    assert_eq!(add(&mut settings, COMMAND).unwrap(), names);
    assert!(add(&mut settings, COMMAND).unwrap().is_empty());
    assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 2);
    assert_eq!(settings["model"], "opus");
    assert_eq!(installed(&settings), names);
    // An older install's background Stop hook becomes one Claude Code
    // waits for.
    settings["hooks"]["Stop"][1]["hooks"][0] = handler(COMMAND, false);
    assert_eq!(add(&mut settings, COMMAND).unwrap(), ["Stop"]);
    assert_eq!(
        settings["hooks"]["Stop"][1]["hooks"][0],
        handler(COMMAND, true)
    );
    assert_eq!(remove(&mut settings), EVENTS.len());
    assert_eq!(
        settings,
        json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
            }
        })
    );
    let mut empty = Value::Null;
    add(&mut empty, COMMAND).unwrap();
    remove(&mut empty);
    assert_eq!(empty, json!({}));
}

#[test]
fn settings_of_the_wrong_shape_are_refused() {
    assert!(add(&mut json!([1]), COMMAND).is_err());
    assert!(add(&mut json!({ "hooks": { "Stop": {} } }), COMMAND).is_err());
}

#[test]
fn the_command_quotes_its_paths() {
    assert_eq!(
        hook_command(Path::new("/opt/it's/beep-node"), Path::new("/srv/node")),
        r"'/opt/it'\''s/beep-node' --dir '/srv/node' hook"
    );
    assert!(is_ours(
        &json!({ "command": hook_command(Path::new("/opt/beep-node"), Path::new("/x")) })
    ));
}
