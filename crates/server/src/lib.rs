pub mod api;
pub mod apns;
pub mod app;
pub mod apps;
pub mod attachments;
pub mod cache_session;
pub mod compaction;
pub mod config;
pub mod credentials;
pub mod cron;
pub mod decision;
pub mod digest;
pub mod events;
pub mod fcm;
pub mod feeds;
pub mod fetch;
pub mod filler;
pub mod followups;
pub mod gates;
pub mod git_items;
pub mod grants;
mod guest;
pub mod images;
pub mod imports;
pub mod instance;
pub mod leak;
pub mod live;
pub mod memories;
pub mod metering;
pub mod models;
pub mod nodes;
pub mod paging;
pub mod permissions;
pub mod places;
pub mod profile;
pub mod projects;
pub mod push;
pub mod remote;
pub mod repeat;
pub mod repo_watch;
pub mod research;
pub mod router;
pub mod search;
pub mod skills;
pub mod sources;
pub mod starters;
pub mod steps;
pub mod store;
pub mod stt;
pub mod subagents;
pub mod timing;
pub mod titles;
pub mod tool_attachments;
pub mod tool_cost;
pub mod tools;
pub mod transport;
pub mod user;
pub mod voice;
pub mod wake;
pub mod watchers;
pub mod workspace;

pub use beep_proto::{self as protocol, client};

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

/// Locks `mutex`, taking its value as it is when a holder panicked: the
/// state the server guards with plain mutexes stays usable after a panic.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
