//! WSL distributions on Windows. The Windows node finds the distributions and
//! their agents for its report (`report`), and runs Claude Code found in one
//! as its own, on the Windows checkout (`agent_command`, `linux_path`). A
//! distribution the user adds as a machine gets a Linux beep-node with its
//! own key and checkouts: the Windows node installs and joins it (`wsl/1`,
//! `Wsl`) and keeps it running (`keeper`).
//!
//! Only a Windows node uses it; the logic builds and is tested everywhere.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

mod distro;
mod keeper;
mod report;
mod serve;
mod steps;

pub(crate) use distro::{DistroName, linux_path, windows_path};
pub(crate) use report::{agent, agents_changed, report};
pub(crate) use steps::{agent_command, home};
pub use serve::Wsl;
