//! What the node knows of the agents it can start: which ones, how their
//! adapters stand, the user's settings for them, and how sessions reach the
//! Beep MCP server and skills.

use super::{
    adapters::installs::Installs,
    harnesses::{Latest, Settings},
    kinds::AgentKind,
    mcp_launch::Launch,
    models::ModelCache,
};
use anyhow::Result;
use beep_proto::node::harness::AgentSettings;
use beep_proto::watched;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{sync::watch, time::timeout};

/// How long resumed sessions wait for the user's agent settings before
/// starting without them.
pub(super) const SETTINGS_WAIT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(crate) struct Catalog {
    /// The agents it can start, replaced when detection finds others.
    pub(super) kinds: watch::Sender<Arc<Vec<AgentKind>>>,
    /// The adapters the node installs itself.
    pub(super) adapters: Installs,
    /// The models each agent offers, found lately.
    pub(super) models: ModelCache,
    /// The user's settings for each agent, applied when one starts.
    pub(super) settings: Settings,
    /// Saved Beep instructions, supplied to sessions without editing agent files.
    pub(super) instructions: Mutex<Option<String>>,
    /// The newest versions of the agents, found lately.
    pub(super) latest: Latest,
    /// Set once the mesh gave the user's agent settings, which resumed
    /// sessions start with.
    pub(super) settled: watch::Sender<bool>,
    /// How sessions start the Beep MCP server, once the node serves it.
    pub(super) mcp: OnceLock<Arc<Launch>>,
    /// Beep's own skills folder, listed for sessions once the node
    /// serves: one made without it, as in tests, lists none.
    pub(super) skills_dir: OnceLock<PathBuf>,
}

impl Catalog {
    pub(crate) fn new(kinds: Vec<AgentKind>) -> Self {
        Self {
            kinds: watch::Sender::new(Arc::new(kinds)),
            ..Self::default()
        }
    }

    /// Gives the sessions started from now on the Beep MCP server, as
    /// `launch` starts it. Only the first call counts.
    pub(crate) fn serve_mcp(&self, launch: Arc<Launch>) {
        // A second launch would give the same server.
        let _ = self.mcp.set(launch);
    }

    /// Lists the skills in Beep's own skills folder for the sessions
    /// started from now on, as a node that serves does. Only the first call
    /// counts.
    pub(crate) fn list_beep_skills(&self) {
        if let Some(dir) = crate::machine::provision::skills_dir() {
            let _ = self.skills_dir.set(dir);
        }
    }

    /// Applies the saved Beep instructions to sessions started or resumed from now on.
    pub(crate) fn set_instructions(&self, text: Option<String>) {
        *crate::lock(&self.instructions) = text;
    }

    /// Takes the user's settings for the agents in place of the ones
    /// before; sessions start with them from now on.
    pub(crate) fn set_settings(&self, settings: AgentSettings) {
        self.settings.set(settings);
    }

    /// The mesh gave the user's settings: sessions held over a restart
    /// may start.
    pub(crate) fn settings_arrived(&self) {
        self.settled.send_replace(true);
    }

    /// Takes the agents a new detection found, when they differ.
    pub(super) fn set_kinds(&self, kinds: Vec<AgentKind>) {
        let changed = self.kinds.send_if_modified(|current| {
            if **current == kinds {
                return false;
            }
            *current = Arc::new(kinds);
            true
        });

        if changed {
            let names: Vec<String> = watched::read_sent(&self.kinds, |kinds| {
                kinds.iter().map(|kind| kind.name.clone()).collect()
            });
            log::info!("coding agents now: {}", names.join(", "));
        }
    }

    /// Readies what `kind` reads before it starts: for flsh, the user's
    /// flsh settings in its config.
    pub(super) async fn prepare(&self, kind: &AgentKind) -> Result<()> {
        let configured = self.settings.configured(kind)?;
        self.settings.prepare(&configured).await
    }

    /// Waits for the user's agent settings for `SETTINGS_WAIT` at most;
    /// whether they came.
    pub(super) async fn settings_came(&self) -> bool {
        let mut settled = self.settled.subscribe();

        timeout(
            SETTINGS_WAIT,
            watched::wait_for(&mut settled, |settled| *settled),
        )
        .await
        .is_ok()
    }
}

#[cfg(test)]
#[path = "../tests/agents_catalog.rs"]
mod tests;
