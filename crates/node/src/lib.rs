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
mod lines;
pub mod link;
pub mod local;
mod process;
pub mod provision;
pub mod service;
pub mod setup;
mod shell;
pub mod state;
pub mod sync;
mod tasks;
pub mod transfer;

pub use tasks::kill_all as kill_tasks;

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;
