//! WSL distributions on Windows. The Windows node finds the distributions and
//! their agents for its report (`report`), and
//! runs Claude Code found in one as its own, on the Windows checkout
//! (`agent_command`, `linux_path`).
//!
//! Only a Windows node uses it; the logic builds and is tested everywhere.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

mod distro;
mod report;
mod steps;

pub(crate) use distro::{DistroName, LinuxPath, linux_path, windows_path};
pub(crate) use report::{agent, agents_changed, report};
pub(crate) use steps::{agent_command, home};
