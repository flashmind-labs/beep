//! What the app remembers between launches, in `state.json`.

use crate::sync;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub link: LinkState,
    /// The chat the bar shows when it opens.
    #[serde(default)]
    pub session: Option<String>,
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
