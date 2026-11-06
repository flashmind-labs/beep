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
//! Unlike the app wire, messages are compact msgpack: structs are arrays read
//! by position (`beep-node/1`, from before it, is the exception and writes
//! maps). Nodes update on their own schedule, so a field may be added at the
//! end with `#[serde(default)]`, or a variant, and a reader built with
//! `lenient::from_slice` skips what it does not know. Never insert, reorder
//! or remove a field.
mod admin;
mod control;
mod env;
mod stream;

pub mod agent;
pub mod desktop;
pub mod files;
pub mod harness;
pub mod mesh_state;
pub mod process;
pub mod skills;
pub mod sync;
pub mod terminal;
pub mod tools;
pub mod transfer;
pub mod usage;
pub mod webhook;

pub use admin::{logs, search, shell, status, tool_update, traffic, update, version};
pub use control::*;
pub use env::*;
pub use stream::*;
