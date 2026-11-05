//! Where this machine keeps a session's transcript.

use super::Harness;
use crate::agents::{Agents, found::Agent};
use std::path::PathBuf;
use tokio::task::spawn_blocking;

impl Agents {
    /// The transcript file of session `id` of `harness` here, when there
    /// is one.
    pub(super) async fn transcript_file(&self, harness: Harness, id: &str) -> Option<PathBuf> {
        let agent = match harness {
            Harness::Claude => return self.external.file(id).await.ok().flatten(),
            Harness::Codex => Agent::Codex,
            Harness::Pi => Agent::Pi,
        };
        let (stores, id) = (self.stores.clone(), id.to_owned());
        spawn_blocking(move || stores.file(Some(agent), &id))
            .await
            .ok()
            .flatten()
    }
}
