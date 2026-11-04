//! beep-server: the agent server behind the Beep apps. `main.rs` starts it; `app::App` is the hub
//! every request handler (`api`) and background task reaches the rest through.

pub mod account;
pub mod agent;
pub mod api;
pub mod app;
pub mod apps;
pub mod config;
pub mod memory;
pub mod nodes;
pub mod projects;
pub mod push;
pub mod research;
pub mod routines;
pub mod store;
pub mod tools;
pub mod voice;

pub use beep_proto::{self as protocol, client};

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

/// Locks `mutex`, taking its value as it is when a holder panicked: the
/// state the server guards with plain mutexes stays usable after a panic.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
