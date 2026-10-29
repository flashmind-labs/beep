//! `shell/1`: a shell on the node. Each stream is one shell that lives as
//! long as the stream, so the folder and variables a command sets carry over
//! to the next. Commands run one at a time.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "shell/1";

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Runs `command` in the shell, with stdin empty and stderr merged into
    /// stdout. After `timeout_secs` the node stops it, and the shell with it.
    Run { command: String, timeout_secs: u32 },
}

/// What the node sends for a `Run`: any number of `Output`, then `Done`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    Output {
        text: String,
    },
    Done {
        /// `None` when the command did not finish: it timed out or ended the
        /// shell.
        exit_code: Option<i32>,
        timed_out: bool,
        /// The shell's folder afterwards, while it still runs.
        cwd: Option<String>,
        /// The command ran in a new shell, started in the home folder,
        /// because there was none yet or the last one ended.
        fresh: bool,
        /// The node dropped output past its limit.
        truncated: bool,
    },
}
