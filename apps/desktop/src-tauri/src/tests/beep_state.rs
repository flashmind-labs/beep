use super::*;

fn parse(json: &str) -> Saved {
    serde_json::from_str(json).expect("the state parses")
}

/// A state file in a fresh directory of its own.
fn state_file(name: &str) -> StateFile {
    let dir = std::env::temp_dir().join(format!("beep-state-{}-{name}", std::process::id()));
    fs::create_dir_all(&dir).expect("the test directory is made");
    let path = dir.join("state.json");

    if path.exists() {
        fs::remove_file(&path).expect("an earlier run's file is removed");
    }

    StateFile::new(path)
}

#[test]
fn zoom_stays_in_range() {
    assert_eq!(parse("{}").zoom.percent(), 100);
    assert_eq!(parse(r#"{"zoom":130}"#).zoom.percent(), 130);
    assert_eq!(parse(r#"{"zoom":900}"#).zoom.percent(), 200);
    assert_eq!(Zoom::new(10).percent(), 70);
}

#[test]
fn a_fresh_install_uses_the_mac() {
    let file = state_file("fresh");
    assert!(file.load().uses_mac());
    file.save(&file.load());
    assert!(file.load().uses_mac());
}

#[test]
fn an_old_file_turns_the_mac_on_once() {
    // A file from before `mac_on`: its false was the old default.
    let file = state_file("old");
    fs::write(&file.0, r#"{"use_mac":false}"#).expect("the state is written");
    let mut saved = file.load();
    assert!(saved.uses_mac());
    file.save(&saved);
    assert!(file.load().uses_mac());
    // Turned off now, it stays off.
    saved.set_use_mac(false);
    file.save(&saved);
    assert!(!file.load().uses_mac());
    assert!(!file.load().use_mac);
}

#[test]
fn a_switch_set_in_this_version_keeps_its_value() {
    let file = state_file("saved");
    fs::write(&file.0, r#"{"use_mac":true,"mac_on":false}"#).expect("the state is written");
    assert!(!file.load().uses_mac());
    fs::write(&file.0, r#"{"use_mac":false,"mac_on":true}"#).expect("the state is written");
    assert!(file.load().uses_mac());
}

#[test]
fn turning_the_mac_on_or_off_sticks() {
    let mut saved = parse(r#"{"use_mac":false}"#);
    saved.set_use_mac(true);
    let json = serde_json::to_string(&saved).expect("the state serializes");
    assert!(parse(&json).uses_mac());
    assert!(parse(&json).use_mac);
    saved.set_use_mac(false);
    let json = serde_json::to_string(&saved).expect("the state serializes");
    assert!(!parse(&json).uses_mac());
    assert!(!parse(&json).use_mac);
}

#[test]
fn agent_notices_are_on_until_turned_off() {
    assert!(parse("{}").agent_notices);
    let mut saved = Saved::default();
    assert!(saved.agent_notices);
    saved.agent_notices = false;
    let json = serde_json::to_string(&saved).expect("the state serializes");
    assert!(!parse(&json).agent_notices);
}

#[test]
fn option_taps_are_on_until_turned_off() {
    assert!(parse("{}").option_taps);
    let saved = Saved {
        option_taps: false,
        ..Saved::default()
    };
    let json = serde_json::to_string(&saved).expect("the state serializes");
    assert!(!parse(&json).option_taps);
}

#[test]
fn forge_notices_are_on_until_turned_off() {
    assert!(parse("{}").forge_notices);
    let saved = Saved {
        forge_notices: false,
        ..Saved::default()
    };
    let json = serde_json::to_string(&saved).expect("the state serializes");
    assert!(!parse(&json).forge_notices);
}

#[test]
fn developer_mode_is_off_until_turned_on() {
    assert!(!parse("{}").developer);
    let saved = Saved {
        developer: true,
        ..Saved::default()
    };
    let json = serde_json::to_string(&saved).expect("the state serializes");
    assert!(parse(&json).developer);
}

#[test]
fn an_older_file_with_server_fields_still_loads() {
    let file = state_file("older");
    fs::write(
        &file.0,
        r#"{"link":"linked","session":"x","signed_out":true,"agent_notices":false,"zoom":120}"#,
    )
    .expect("the state is written");
    let saved = file.load();
    assert!(!saved.agent_notices);
    assert_eq!(saved.zoom.percent(), 120);
}

#[test]
fn node_release_channel_follows_developer_mode_changes() {
    use beep_proto::node::update::ReleaseChannel;

    let prefs = Prefs::load(state_file("node-channel"));
    assert_eq!(prefs.node_release_channel(), ReleaseChannel::Minor);
    prefs.set_developer(true);
    assert_eq!(prefs.node_release_channel(), ReleaseChannel::All);
    prefs.set_developer(false);
    assert_eq!(prefs.node_release_channel(), ReleaseChannel::Minor);
}
