//! WSL distributions as machines of their own, on Windows. Coding agents
//! installed in WSL run there, not on Windows, so a distribution the user
//! adds gets a Linux beep-node with its own key and checkouts, joined to the
//! mesh like any node. The Windows node finds the distributions and their
//! agents for its report (`report`), installs and joins a node in one
//! (`wsl/1`, `Wsl`), and keeps those nodes running (`keeper`).
//!
//! Only a Windows node uses it; the logic builds and is tested everywhere.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

mod distro;
mod keeper;
mod report;
mod serve;
mod steps;

pub(crate) use report::report;
pub use serve::Wsl;
