//! `process/1`: commands a node runs in the background, apart from any
//! stream, so they outlive the turn that started them. Each stream carries
//! one `Request` and its `Reply`. The node keeps a bounded number of tasks
//! and the last lines of each; stopping one stops its whole process group.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL: &str = "process/1";

/// The most tasks a node keeps, running or finished.
pub const MAX_TASKS: usize = 32;
/// The most output lines a node keeps per task.
pub const MAX_LINES: usize = 100;

/// A task's number on its node, unique while the node runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub u32);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Runs `command` with bash (or sh), stdin empty and stderr merged into
    /// stdout, in `cwd` or else the home folder.
    Start {
        command: String,
        #[serde(default)]
        cwd: Option<String>,
    },
    List {},
    Status {
        id: TaskId,
    },
    /// Stops the task's process group, gently then hard, and answers with
    /// its status.
    Stop {
        id: TaskId,
    },
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    Started {
        id: TaskId,
    },
    Tasks {
        tasks: Vec<Task>,
    },
    Status {
        task: Task,
        /// The last lines of output, oldest first.
        lines: Vec<String>,
        /// Lines were dropped before these.
        #[serde(default)]
        earlier: bool,
    },
    /// The request failed: no such task, no room for another, or the
    /// command could not start.
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub command: String,
    pub state: TaskState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TaskState {
    Running,
    /// It ended on its own; `code` is `None` when a signal ended it.
    Exited {
        code: Option<i32>,
    },
    /// It was stopped.
    Stopped,
    #[serde(other)]
    Unknown,
}

impl TaskState {
    pub fn is_running(self) -> bool {
        self == Self::Running
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => f.write_str("running"),
            Self::Exited { code: Some(0) } => f.write_str("completed"),
            Self::Exited { code: Some(code) } => write!(f, "failed (exit code {code})"),
            Self::Exited { code: None } => f.write_str("ended by a signal"),
            Self::Stopped => f.write_str("stopped"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}
