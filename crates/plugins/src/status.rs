use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use crate::manifest::PluginId;
use crate::meter::Meter;

/// Where a plugin stands, as the page and agent tools read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
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
}

/// One plugin's name, state and cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub id: PluginId,
    pub name: String,
    pub version: semver::Version,
    #[serde(flatten)]
    pub state: PluginState,
    /// Script time over the last minute, in ms.
    pub used_ms: u64,
    pub pending_timers: usize,
}

/// A plugin's life as the supervisor tracks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    Waiting,
    Running,
    Stopped(String),
}

struct Entry {
    name: String,
    version: semver::Version,
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
        name: String,
        version: semver::Version,
        lifecycle: Lifecycle,
        meter: Option<Arc<Meter>>,
    ) {
        self.entries.insert(
            id,
            Entry {
                name,
                version,
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

    pub(crate) fn list(&self, now: Instant) -> Vec<PluginStatus> {
        self.entries
            .iter()
            .map(|(id, entry)| {
                let state = match &entry.lifecycle {
                    Lifecycle::Waiting => PluginState::Waiting,
                    Lifecycle::Running if self.visible => PluginState::Running,
                    Lifecycle::Running => PluginState::Paused,
                    Lifecycle::Stopped(reason) => PluginState::Stopped {
                        reason: reason.clone(),
                    },
                };
                let (used, timers) = entry.meter.as_ref().map_or((0, 0), |meter| {
                    (meter.used(now).as_millis(), meter.timers())
                });

                PluginStatus {
                    id: id.clone(),
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    state,
                    used_ms: u64::try_from(used).unwrap_or(u64::MAX),
                    pending_timers: timers,
                }
            })
            .collect()
    }
}
