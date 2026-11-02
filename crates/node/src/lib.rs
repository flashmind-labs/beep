//! `beep-node`: runs on a machine so Beep can run work on it.
//!
//! `beep-node setup <token>` makes the machine's key and claims a setup
//! token from Beep, which links the key to the user's account, then installs
//! `beep-node run` as a background service (`service`); the older
//! `beep-node link` prints a link code for the phone instead. `beep-node run`
//! keeps one connection to the server, reports what the machine has and
//! serves the sessions Beep opens. `beep-node local` does both for the node
//! bundled with a single-user server, which links it to the owner itself.

pub mod agents;
pub mod claude;
pub mod env;
pub mod files;
mod found;
mod fsat;
mod lines;
pub mod link;
pub mod local;
mod process;
pub mod provision;
pub mod service;
pub mod setup;
mod shell;
pub mod state;
mod stats;
pub mod sync;
pub mod syncthing;
mod tasks;
mod terminal;
pub mod transfer;

/// Kills every background task and terminal still running, at once. For
/// a host about to exit, such as the Mac app: a process exit drops nothing,
/// so they would outlive it.
pub fn kill_tasks() {
    tasks::kill_all();
    terminal::kill_all();
}

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;
