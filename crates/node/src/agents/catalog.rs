//! What the node knows of the agents it can start: which ones, how their
//! adapters stand, the user's settings for them, and how sessions reach the
//! Beep MCP server and skills.

use super::{
    adapters::installs::Installs,
    harnesses::{self, Latest, Settings},
    kinds::AgentKind,
    mcp_launch::Launch,
    models::ModelCache,
};
use anyhow::Result;
use crate::process::{self, spawn_env::{self, Scope}};
use beep_proto::node::harness::{AgentSettings, HarnessReport};
use beep_proto::AccountName;
use beep_proto::watched;
use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, time::timeout};

/// How long resumed sessions wait for the user's agent settings before
/// starting without them.
pub(super) const SETTINGS_WAIT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Catalog {
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
    /// How sessions start the Beep MCP server: `None` for a node that does
    /// not serve it, as in tests.
    pub(super) mcp: Option<Arc<Launch>>,
    /// Beep's own skills folder, listed for sessions: `None` lists none.
    pub(super) skills_dir: Option<PathBuf>,
}

impl Catalog {
    pub(crate) fn new(
        kinds: Vec<AgentKind>,
        mcp: Option<Arc<Launch>>,
        skills_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            kinds: watch::Sender::new(Arc::new(kinds)),
            mcp,
            skills_dir,
            ..Self::default()
        }
    }

    /// The CLI of agent `name`, with `args`, as a plan read starts it: signed
    /// in as the agent's sessions on `account` are, with the user's settings
    /// for it. `None` when this node does not run it or the user turned it off.
    pub(super) fn cli(
        &self,
        name: &str,
        account: Option<&AccountName>,
        args: &[&str],
    ) -> Option<tokio::process::Command> {
        let kind = watched::read_sent(&self.kinds, |kinds| {
            kinds.iter().find(|kind| kind.name == name).cloned()
        })?;
        let kind = self.settings.configured(&kind).ok()?;
        let program = harnesses::find(name)?;
        let mut command = process::command(program);
        spawn_env::apply(&mut command, Scope::for_agent(name, account));
        command
            .args(args)
            .envs(&kind.env)
            .current_dir(env::temp_dir());
        Some(command)
    }

    /// What this node finds of each agent harness it knows.
    pub(super) async fn harnesses(&self, refresh: bool) -> HarnessReport {
        let kinds = watched::read_sent(&self.kinds, Clone::clone);

        harnesses::report(&kinds, &self.settings, &self.latest, refresh).await
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
