//! `beep-node`: runs on a machine so Beep can run work on it.
//!
//! `beep-node link` makes the machine's key and prints a link code; once the
//! phone takes it, the key signs into the user's account. `beep-node run`
//! then keeps one connection to the server, reports what the machine has and
//! serves the sessions Beep opens.

pub mod agents;
pub mod claude;
pub mod env;
pub mod files;
pub mod link;
mod process;
pub mod service;
mod shell;
pub mod state;
