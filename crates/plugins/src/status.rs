use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use crate::managed::Origin;
use crate::manifest::{Permission, PluginId};
use crate::meter::Meter;
use crate::style::Style;
use crate::tool::ToolSpec;

/// Where a plugin stands, as the page and agent tools read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum PluginState {
    /// Not started: nothing it is activated by has happened.
    Waiting,
    Running,
    /// Running, but no window is visible, so its timers and updates wait.
    Paused,
    /// Failed, went over its budget or has an invalid manifest. It shows
    /// nothing until its files change.
    Stopped {
        reason: String,
    },
    /// Turned off by the user.
    Disabled,
    /// The manifest asks for permissions the user has not allowed. It does
    /// not run until they are.
    NeedsApproval {
        /// The permissions that are not allowed yet.
        missing: Vec<Permission>,
    },
}

/// One plugin's name, state and cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub id: PluginId,
    pub name: String,
    pub version: semver::Version,
    pub description: String,
    /// What the manifest asks for.
    pub permissions: Vec<Permission>,
    /// What the user allowed.
    pub approved: Vec<Permission>,
    pub source: Origin,
    /// The tools the manifest gives coding agents.
    pub tools: Vec<ToolSpec>,
    /// The style tokens the manifest sets.
    pub style: Style,
    /// Whether the user left it turned on.
    pub enabled: bool,
    #[serde(flatten)]
    pub state: PluginState,
    /// CPU time over the last minute, in ms.
    pub used_ms: u64,
    pub pending_timers: usize,
}

/// A plugin's life as the supervisor tracks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    Waiting,
    Running,
    Stopped(String),
    Disabled,
    NeedsApproval(Vec<Permission>),
}

/// What a plugin's manifest and record say about it.
pub(crate) struct Info {
    pub name: String,
    pub version: semver::Version,
    pub description: String,
    pub permissions: Vec<Permission>,
    pub approved: Vec<Permission>,
    pub source: Origin,
    pub tools: Vec<ToolSpec>,
    pub style: Style,
    pub enabled: bool,
}

struct Entry {
    info: Info,
    lifecycle: Lifecycle,
    meter: Option<Arc<Meter>>,
}

/// Every plugin's status. The supervisor writes it and the host reads it.
#[derive(Default)]
pub(crate) struct Registry {
    visible: bool,
    entries: BTreeMap<PluginId, Entry>,
}

impl Registry {
    pub(crate) fn set(
        &mut self,
        id: PluginId,
        info: Info,
        lifecycle: Lifecycle,
        meter: Option<Arc<Meter>>,
    ) {
        self.entries.insert(
            id,
            Entry {
                info,
                lifecycle,
                meter,
            },
        );
    }

    /// Moves a known plugin to another lifecycle, keeping its name and meter.
    pub(crate) fn move_to(&mut self, id: &PluginId, lifecycle: Lifecycle) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.lifecycle = lifecycle;
        }
    }

    /// Shows the meter of a plugin's current run.
    pub(crate) fn attach(&mut self, id: &PluginId, meter: Arc<Meter>) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.meter = Some(meter);
        }
    }

    pub(crate) fn remove(&mut self, id: &PluginId) {
        self.entries.remove(id);
    }

    pub(crate) fn lifecycle(&self, id: &PluginId) -> Option<&Lifecycle> {
        self.entries.get(id).map(|entry| &entry.lifecycle)
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// The list as `list` gives it, with no CPU time or timers, so two
    /// reads differ only when a plugin or its state did.
    pub(crate) fn list_without_cost(&self) -> Vec<PluginStatus> {
        let mut all = self.list(Instant::now());
        for status in &mut all {
            status.used_ms = 0;
            status.pending_timers = 0;
        }

        all
    }

    /// A plugin's state as the page and agent tools read it.
    pub(crate) fn state(&self, id: &PluginId) -> Option<PluginState> {
        self.entries.get(id).map(|entry| self.state_of(entry))
    }

    fn state_of(&self, entry: &Entry) -> PluginState {
        match &entry.lifecycle {
            Lifecycle::Waiting => PluginState::Waiting,
            Lifecycle::Running if self.visible => PluginState::Running,
            Lifecycle::Running => PluginState::Paused,
            Lifecycle::Stopped(reason) => PluginState::Stopped {
                reason: reason.clone(),
            },
            Lifecycle::Disabled => PluginState::Disabled,
            Lifecycle::NeedsApproval(missing) => PluginState::NeedsApproval {
                missing: missing.clone(),
            },
        }
    }

    pub(crate) fn list(&self, now: Instant) -> Vec<PluginStatus> {
        self.entries
            .iter()
            .map(|(id, entry)| {
                let (used, timers) = entry.meter.as_ref().map_or((0, 0), |meter| {
                    (meter.used(now).as_millis(), meter.timers())
                });

                PluginStatus {
                    id: id.clone(),
                    name: entry.info.name.clone(),
                    version: entry.info.version.clone(),
                    description: entry.info.description.clone(),
                    permissions: entry.info.permissions.clone(),
                    approved: entry.info.approved.clone(),
                    source: entry.info.source.clone(),
                    tools: if cfg!(feature = "tools") {
                        entry.info.tools.clone()
                    } else {
                        Vec::new()
                    },
                    style: entry.info.style.clone(),
                    enabled: entry.info.enabled,
                    state: self.state_of(entry),
                    used_ms: u64::try_from(used).unwrap_or(u64::MAX),
                    pending_timers: timers,
                }
            })
            .collect()
    }
}

impl fmt::Display for PluginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Waiting => f.write_str("waiting to start"),
            Self::Running => f.write_str("running"),
            Self::Paused => f.write_str("paused while no Beep window is visible"),
            Self::Stopped { reason } => write!(f, "stopped ({reason})"),
            Self::Disabled => f.write_str("turned off"),
            Self::NeedsApproval { .. } => {
                f.write_str("waiting for the user's approval in Settings, Plugins")
            }
        }
    }
}
