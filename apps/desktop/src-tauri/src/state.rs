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
    /// The "Let Beep use this Mac" switch from before `mac_off`. It was
    /// false for everyone, never a choice, so it no longer decides; it is
    /// still written so an older app reads a matching value.
    #[serde(default)]
    pub use_mac: bool,
    /// The user turned "Let Beep use this Mac" off. Unless they did, the
    /// Mac serves as a node with desktop control.
    #[serde(default)]
    pub mac_off: bool,
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

    /// "Let Beep use this Mac": on unless the user turned it off.
    pub fn uses_mac(&self) -> bool {
        !self.mac_off
    }

    pub fn set_use_mac(&mut self, on: bool) {
        self.mac_off = !on;
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
            mac_off: false,
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
        fs::read(&self.0)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
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

    #[test]
    fn the_mac_is_used_unless_turned_off() {
        assert!(parse("{}").uses_mac());
        // Every saved state had this, though nobody chose it.
        assert!(parse(r#"{"use_mac":false}"#).uses_mac());
        assert!(!parse(r#"{"use_mac":false,"mac_off":true}"#).uses_mac());
        let mut saved = parse(r#"{"use_mac":false}"#);
        saved.set_use_mac(false);
        assert!(!saved.uses_mac());
        saved.set_use_mac(true);
        assert!(saved.uses_mac());
        assert!(saved.use_mac);
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
