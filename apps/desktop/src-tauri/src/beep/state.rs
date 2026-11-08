//! What the app remembers between launches, in `state.json`.

use crate::beep::sync;
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

/// The page zoom of every window, in percent, from 70 to 200.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Zoom(u16);

impl Zoom {
    const MIN: u16 = 70;
    const MAX: u16 = 200;

    /// `percent`, brought into range.
    pub fn new(percent: u16) -> Self {
        Self(percent.clamp(Self::MIN, Self::MAX))
    }

    /// In range even when a hand-edited file says otherwise.
    pub fn percent(self) -> u16 {
        self.0.clamp(Self::MIN, Self::MAX)
    }

    /// The webview's zoom factor, 1.0 at 100%.
    pub fn factor(self) -> f64 {
        f64::from(self.percent()) / 100.0
    }
}

impl Default for Zoom {
    fn default() -> Self {
        Self(100)
    }
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
    /// Set only once the user picks one in Settings.
    #[serde(default)]
    pub notice_voice: Option<NoticeVoice>,
    /// In live mode a pause sends; otherwise only saying "send" does.
    #[serde(default = "on")]
    pub send_on_pause: bool,
    /// Chats live mode talked in, newest last.
    #[serde(default)]
    pub live_chats: Vec<Uuid>,
    /// Coding agents notify when they ask something or finish a turn.
    #[serde(default = "on")]
    pub agent_notices: bool,
    /// Machines removed from the mesh that the server may still list. Each
    /// stays until the server dropped it, and is retried at launch.
    #[serde(default)]
    pub unlinks: Vec<String>,
    /// Signed out of the Beep server from Settings: the key and `link` stay,
    /// and this Mac makes no server calls until it signs in again.
    #[serde(default)]
    pub signed_out: bool,
    /// Page zoom of every window, ⌘= ⌘- ⌘0.
    #[serde(default)]
    pub zoom: Zoom,
}

fn on() -> bool {
    true
}

impl Saved {
    /// The Voice notes choice. Without one a notice is only a notification.
    pub fn notice_voice(&self) -> NoticeVoice {
        self.notice_voice.unwrap_or(NoticeVoice::Off)
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

    /// Notes that the server still has to drop `machine`.
    pub fn owe_unlink(&mut self, machine: &str) {
        if !self.unlinks.iter().any(|owed| owed == machine) {
            self.unlinks.push(machine.to_owned());
        }
    }

    /// The server dropped `machine`, or never had it.
    pub fn settle_unlink(&mut self, machine: &str) {
        self.unlinks.retain(|owed| owed != machine);
    }

    /// Where the link stands as the rest of the app sees it: a Mac signed
    /// out of the server is unlinked until it signs in, with its key kept.
    pub fn effective_link(&self) -> LinkState {
        if self.signed_out {
            LinkState::Unlinked
        } else {
            self.link
        }
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
            agent_notices: true,
            unlinks: Vec::new(),
            signed_out: false,
            zoom: Zoom::default(),
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
#[path = "../tests/beep_state.rs"]
mod tests;
