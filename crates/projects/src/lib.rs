//! Project code the server and the desktop share: the shell commands for git and host work, the
//! parsers for their output, and the names they use.

pub mod commands;
pub mod leak;
pub mod model;
mod owner_var;
mod shell;

pub use owner_var::github_owner_var;
pub use shell::{CommandOutput, Shell, quote};
