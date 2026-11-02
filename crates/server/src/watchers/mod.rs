//! Watchers: after a conversation or a task, separate agents read it over,
//! each with its own job. The memory watcher stores, updates and forgets
//! memories, each filed under what it is about. The skills watcher turns
//! steps the user repeats into skills. The improvement watcher notes where
//! Beep failed the user, as findings the user resolves or dismisses; it
//! never changes anything itself.
//!
//! A chat is ready for them once it has new messages and has been quiet for
//! a while, or straight away when a task in it ended: a routine run, an
//! event's wake, a coding
//! session's turn report (see [`trigger`]). Each watcher keeps its own place
//! in each chat, the last message it read, so nothing is read twice and a
//! restart picks up where it stopped. One trigger fans out to every watcher,
//! each running on its own with its own model, timeout and failures, a few
//! at a time across the server (see [`runner`]). Their model calls are
//! metered to the chat's user. Skills and improvement skip a stretch with
//! too little in it and just move their place past it (see [`gate`]).
//!
//! Every user's chats are read, each for its own user.

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
