//! What the app remembers between launches, in `state.json`.

use crate::sync;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkState {
    /// Showing a link code. The app does not connect, so the server never
    /// makes an account for this key.
    #[default]
    Unlinked,
    Linked,
    /// The phone removed this computer.
    Revoked,
}

/// What a new notice does while the user is at this Mac and not on a call.
/// Otherwise it is only a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeVoice {
    /// The orb shows a card with Play and Open.
    Ask,
    /// It is read aloud right away.
    Aloud,
    /// Only the notification.
    Off,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub link: LinkState,
    /// The chat the bar shows when it opens.
    #[serde(default)]
    pub session: Option<String>,
    /// Chats started here as researches, which open in their own window.
    #[serde(default)]
    pub researches: Vec<Uuid>,
    /// The "Let Beep use this Mac" switch from before `mac_on`, still
    /// written so an older app reads a matching value. It was off unless
    /// turned on, not a choice, so it no longer decides.
    #[serde(default)]
    pub use_mac: bool,
    /// "Let Beep use this Mac" as this app version saved it. Missing in
    /// files from before it, which start with the Mac on once, as a fresh
    /// install does.
    #[serde(default)]
    pub mac_on: Option<bool>,
    /// The Voice notes switch from before `notice_voice`, still written so
    /// an older app reads a matching value.
    #[serde(default = "on")]
    pub voice_notes: bool,
    /// What a new notice does while the user is here and not on a call.
    /// Missing in files from before it, where `voice_notes` decides.
    #[serde(default)]
    pub notice_voice: Option<NoticeVoice>,
    /// In live mode a pause sends; otherwise only saying "send" does.
    #[serde(default = "on")]
    pub send_on_pause: bool,
    /// Chats live mode talked in, newest last.
    #[serde(default)]
    pub live_chats: Vec<Uuid>,
}

fn on() -> bool {
    true
}

impl Saved {
    /// The Voice notes choice. The old switch on becomes Ask first, off
    /// stays Off.
    pub fn notice_voice(&self) -> NoticeVoice {
        match self.notice_voice {
            Some(choice) => choice,
            None if self.voice_notes => NoticeVoice::Ask,
            None => NoticeVoice::Off,
        }
    }

    /// "Let Beep use this Mac": on unless turned off in this version.
    pub fn uses_mac(&self) -> bool {
        self.mac_on.unwrap_or(true)
    }

    /// Brings a file from an older app up to date. Its `use_mac` false was
    /// the old default rather than a choice, so the Mac starts on.
    fn migrate(&mut self) {
        if self.mac_on.is_none() {
            self.set_use_mac(true);
        }
    }

    pub fn set_use_mac(&mut self, on: bool) {
        self.mac_on = Some(on);
        self.use_mac = on;
    }

    pub fn set_notice_voice(&mut self, choice: NoticeVoice) {
        self.notice_voice = Some(choice);
        self.voice_notes = choice != NoticeVoice::Off;
    }
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            link: LinkState::default(),
            session: None,
            researches: Vec::new(),
            use_mac: true,
            mac_on: Some(true),
            voice_notes: true,
            notice_voice: None,
            send_on_pause: true,
            live_chats: Vec::new(),
        }
    }
}

pub struct StateFile(PathBuf);

impl StateFile {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn load(&self) -> Saved {
        let mut saved: Saved = fs::read(&self.0)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        saved.migrate();
        saved
    }

    pub fn save(&self, saved: &Saved) {
        let result = serde_json::to_vec_pretty(saved)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| sync::write(&self.0, &bytes));
        if let Err(e) = result {
            log::warn!("saving {}: {e:#}", self.0.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Saved {
        serde_json::from_str(json).expect("the state parses")
    }

    #[test]
    fn the_old_voice_notes_switch_becomes_a_choice() {
        assert_eq!(parse("{}").notice_voice(), NoticeVoice::Ask);
        assert_eq!(
            parse(r#"{"voice_notes":true}"#).notice_voice(),
            NoticeVoice::Ask
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
    fn a_choice_keeps_the_old_switch_in_step() {
        let mut saved = Saved::default();
        saved.set_notice_voice(NoticeVoice::Off);
        assert!(!saved.voice_notes);
        saved.set_notice_voice(NoticeVoice::Aloud);
        assert!(saved.voice_notes);
        let json = serde_json::to_string(&saved).expect("the state serializes");
        assert_eq!(parse(&json).notice_voice(), NoticeVoice::Aloud);
    }
}
