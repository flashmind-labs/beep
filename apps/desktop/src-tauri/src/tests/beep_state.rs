use super::*;

fn parse(json: &str) -> Saved {
    serde_json::from_str(json).expect("the state parses")
}

#[test]
fn a_notice_is_only_a_notification_until_the_user_picks_otherwise() {
    assert_eq!(parse("{}").notice_voice(), NoticeVoice::Off);
    assert_eq!(
        parse(r#"{"voice_notes":true}"#).notice_voice(),
        NoticeVoice::Off
    );
    assert_eq!(
        parse(r#"{"voice_notes":false}"#).notice_voice(),
        NoticeVoice::Off
    );
    assert_eq!(
        parse(r#"{"voice_notes":true,"notice_voice":"aloud"}"#).notice_voice(),
        NoticeVoice::Aloud
    );
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
fn an_unlink_is_kept_once_until_settled() {
    let mut saved = parse("{}");
    saved.owe_unlink("a");
    saved.owe_unlink("a");
    saved.owe_unlink("b");
    assert_eq!(saved.unlinks, ["a", "b"]);
    saved.settle_unlink("a");
    assert_eq!(saved.unlinks, ["b"]);
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
fn a_signed_out_mac_is_unlinked_and_stays_so_across_launches() {
    assert!(!parse("{}").signed_out);
    assert_eq!(
        parse(r#"{"link":"linked"}"#).effective_link(),
        LinkState::Linked
    );
    let file = state_file("signed-out");
    let mut saved = file.load();
    saved.link = LinkState::Linked;
    saved.signed_out = true;
    file.save(&saved);
    // The launch after: still signed out, and the link kept for signing in.
    let again = file.load();
    assert!(again.signed_out);
    assert_eq!(again.link, LinkState::Linked);
    assert_eq!(again.effective_link(), LinkState::Unlinked);
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
fn a_choice_keeps_the_old_switch_in_step() {
    let mut saved = Saved::default();
    saved.set_notice_voice(NoticeVoice::Off);
    assert!(!saved.voice_notes);
    saved.set_notice_voice(NoticeVoice::Aloud);
    assert!(saved.voice_notes);
    let json = serde_json::to_string(&saved).expect("the state serializes");
    assert_eq!(parse(&json).notice_voice(), NoticeVoice::Aloud);
}
