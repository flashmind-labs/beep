//! WSL distributions on Windows. The Windows node finds the distributions and
//! their agents for its report (`report`), and runs the coding agents found
//! in one as its own, on the Windows checkout: Claude Code on the node's
//! bridge, the others through `relay`, both turning paths across
//! (`translate`).
//!
//! Only a Windows node uses it; the logic builds and is tested everywhere.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

mod distro;
mod relay;
mod report;
mod steps;

pub use distro::{DistroName, LinuxPath, NameError, NotLinuxPath};
pub(crate) use distro::{linux_path, windows_drive_path, windows_folder, windows_path};
pub(crate) use relay::{Toward, relay, translate};
pub(crate) use report::{agents_changed, found, report, with_agent};
pub(crate) use steps::{agent_command, joined_wslenv};
