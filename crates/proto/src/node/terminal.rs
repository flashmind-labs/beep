//! `terminal/1`: interactive terminals on a node. Each runs a command, or
//! the user's shell, in a pseudo-terminal the node owns, so it outlives the
//! stream that started it and anyone can type into it.
//!
//! A stream opens with one `Request`. `Start`, `List` and `Stop` get one
//! `Reply` each, like `process/1`. `Attach` turns the stream into a live
//! view: the node sends `Output` from the asked offset on, and the server
//! may send `Input` and `Resize` until it closes the stream or the node
//! sends `Ended`.

use super::process::{Task, TaskId};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

pub const PROTOCOL: &str = "terminal/1";

/// The most terminals a node keeps, running or finished.
pub const MAX_TERMINALS: usize = 16;
/// The most output a node keeps per terminal, in bytes.
pub const MAX_SCROLLBACK: usize = 256 * 1024;
/// The most one `Input` may carry, in bytes.
pub const MAX_INPUT: usize = 64 * 1024;
/// The most one `Output` carries, in bytes.
pub const MAX_OUTPUT: usize = 32 * 1024;

/// A terminal's size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl Size {
    /// The size a terminal starts with when nobody says.
    pub const DEFAULT: Size = Size { cols: 80, rows: 24 };

    /// `cols` by `rows`, each kept between 2 and 500.
    pub fn clamped(cols: u32, rows: u32) -> Self {
        let fit = |n: u32| u16::try_from(n.clamp(2, 500)).unwrap_or(80);
        Self {
            cols: fit(cols),
            rows: fit(rows),
        }
    }
}

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Runs `command` with the user's shell in a new pseudo-terminal, or
    /// the shell itself, interactive, when `command` is `None`, in `cwd`
    /// or else the home folder.
    Start {
        command: Option<String>,
        #[serde(default)]
        cwd: Option<String>,
        size: Size,
    },
    List {},
    /// Ends the terminal's processes, gently then hard, and answers with
    /// how it ended.
    Stop {
        id: TaskId,
    },
    /// Follows terminal `id` from byte `from` of its output.
    Attach {
        id: TaskId,
        from: u64,
    },
    /// Keys typed into the attached terminal.
    Input {
        data: ByteBuf,
    },
    /// The attached terminal's new size.
    Resize {
        size: Size,
    },
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// The new terminal; its `command` is the shell's name when it runs
    /// the shell itself.
    Started {
        task: Task,
    },
    Terminals {
        terminals: Vec<Task>,
    },
    Stopped {
        task: Task,
    },
    /// Output starting at byte `at`. An `at` past the offset asked for
    /// means the node no longer kept the bytes in between.
    Output {
        at: u64,
        data: ByteBuf,
    },
    /// The attached terminal ended; nothing follows.
    Ended {
        task: Task,
    },
    /// The request failed: no such terminal, no room for another, or the
    /// command could not start.
    Error {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::process::TaskState;

    #[test]
    fn requests_and_replies_round_trip() {
        let request = Request::Start {
            command: None,
            cwd: Some("/srv/project".into()),
            size: Size {
                cols: 120,
                rows: 40,
            },
        };
        let body = rmp_serde::to_vec_named(&request).unwrap();
        assert_eq!(rmp_serde::from_slice::<Request>(&body).unwrap(), request);
        let reply = Reply::Ended {
            task: Task {
                id: TaskId(4),
                command: "bash".into(),
                state: TaskState::Exited { code: Some(0) },
            },
        };
        let body = rmp_serde::to_vec_named(&reply).unwrap();
        assert_eq!(rmp_serde::from_slice::<Reply>(&body).unwrap(), reply);
    }

    #[test]
    fn sizes_are_kept_sane() {
        assert_eq!(Size::clamped(0, 100_000), Size { cols: 2, rows: 500 });
        assert_eq!(
            Size::clamped(132, 43),
            Size {
                cols: 132,
                rows: 43
            }
        );
    }
}
