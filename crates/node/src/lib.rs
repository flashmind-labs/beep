//! `beep-node`: runs on a machine so Beep can run work on it.
//!
//! `beep-node link` makes the machine's key and prints a link code; once the
//! phone takes it, the key signs into the user's account. `beep-node run`
//! then keeps one connection to the server, reports what the machine has and
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
pub mod service;
pub mod setup;
mod shell;
pub mod state;
mod tasks;
pub mod transfer;

pub use tasks::kill_all as kill_tasks;
