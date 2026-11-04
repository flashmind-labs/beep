//! Project code the server and the desktop share: the shell commands for git and host work, the
//! parsers for their output, and the names they use.

pub mod commands;
pub mod git_items;
pub mod imports;
pub mod issues;
pub mod leak;
pub mod model;
mod owner_var;
pub mod repo_watch;
mod shell;

pub use owner_var::github_owner_var;
pub use shell::{Clip, Shell, ShellOutput, quote};
