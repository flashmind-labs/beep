//! The machine itself: what it has (`env`, `stats`), installing and running
//! the node (`setup`, `service`, `update`, `provision`), adding machines over
//! `ssh`, and repository webhooks (`webhook`).

pub mod env;
pub mod provision;
pub mod service;
pub mod setup;
pub(crate) mod ssh;
pub(crate) mod stats;
#[cfg(feature = "cli")]
pub mod update;
pub mod webhook;
