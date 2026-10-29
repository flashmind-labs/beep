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
    /// "Let Beep use this Mac": serve as a node with desktop control.
    #[serde(default)]
    pub use_mac: bool,
    /// Speak new notices aloud while the user is here and not on a call.
    #[serde(default = "on")]
    pub voice_notes: bool,
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

impl Default for Saved {
    fn default() -> Self {
        Self {
            link: LinkState::default(),
            session: None,
            researches: Vec::new(),
            use_mac: false,
            voice_notes: true,
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
