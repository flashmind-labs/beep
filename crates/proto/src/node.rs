//! The messages a node serves its peers: a machine running `beep-node`
//! that Beep runs work on.
//!
//! Work runs in sessions, each a bi stream a peer opens that starts with the
//! name of the protocol it speaks, one the node offers.
//!
//! Messages are compact msgpack: structs are arrays read by position. Nodes
//! update on their own schedule, so a field may be added at the end with
//! `#[serde(default)]`, or a variant, and a reader built with
//! `lenient::from_slice` skips what it does not know. Never insert, reorder
//! or remove a field.
mod admin;
pub mod commands;
mod control;
mod env;
mod stream;
pub mod subscriptions;

pub mod agent;
pub mod desktop;
pub mod device;
pub mod device_video;
pub mod events;
pub mod files;
pub mod forward;
pub mod harness;
pub mod mesh_state;
pub mod metrics;
pub mod process;
pub mod report;
pub mod sign_in;
pub mod skill_library;
pub mod skills;
pub mod sync;
pub mod terminal;
pub mod tools;
pub mod transfer;
pub mod usage;

pub use admin::{logs, search, setup, shell, status, tool_update, traffic, update, version, wsl};
pub use control::*;
pub use env::*;
pub use stream::*;
