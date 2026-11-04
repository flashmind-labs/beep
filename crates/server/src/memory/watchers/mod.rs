//! Watchers: agents that read a finished conversation or task for memories, skills and improvements.
//! Each keeps its own place per chat, so nothing is read twice and restarts resume.

mod config;
mod gate;
pub(crate) mod improvement;
mod memory;
pub(crate) mod runner;
mod skills;
#[cfg(test)]
pub(crate) mod testing;
mod transcript;
pub(crate) mod trigger;

pub use config::{WatcherConfig, WatchersConfig};
pub use improvement::{Cause, FindingKind, FindingStatus};
pub use runner::{Hub, Settings, TimingSettings, WatcherSettings, start};
pub(crate) use trigger::{ChatActivity, WatcherState};

use strum::{EnumString, IntoStaticStr};

/// Each watcher, by the name its place is kept under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum WatcherKind {
    Memory,
    Skills,
    Improvement,
}

impl WatcherKind {
    pub const ALL: [WatcherKind; 3] = [Self::Memory, Self::Skills, Self::Improvement];
}
