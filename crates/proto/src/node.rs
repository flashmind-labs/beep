//! The link between the server and a node: a machine running `beep-node`
//! that Beep runs work on.
//!
//! A node keeps one connection open on `NODE_ALPN`, signed in with its key
//! like any device. The first bi stream it opens is the control stream: it
//! sends a `NodeHello`, the server answers `Down::Welcome`, and from then on
//! the node sends `Up` messages and the server `Down` ones. Work runs in
//! sessions, each a bi stream the server opens that starts with the name of
//! the protocol it speaks, one of the hello's `offers`.
//!
//! Unlike the app wire, messages are msgpack maps with named fields. Nodes
//! update on their own schedule, so either side may add a field (with
//! `#[serde(default)]`) or a variant, and the other skips what it does not
//! know.
mod control;
mod env;
mod stream;

pub mod agent;
pub mod desktop;
pub mod files;
pub mod harness;
pub mod logs;
pub mod process;
pub mod search;
pub mod shell;
pub mod ssh;
pub mod sync;
pub mod terminal;
pub mod tool_update;
pub mod tools;
pub mod traffic;
pub mod transfer;
pub mod update;
pub mod usage;
pub mod webhook;

pub use control::*;
pub use env::*;
pub use stream::*;
