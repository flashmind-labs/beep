#![allow(dead_code, reason = "the project agent uses these in the next tasks")]

//! Projects: a folder of work with its own agent, whose personas live in
//! Markdown files next to the code.

use crate::{store::TaskRow, user::User};
use anyhow::Result;
use model::TaskState;

pub use service::Projects;

mod commands;
pub(crate) mod fleet;
pub(crate) mod gate;
pub(crate) mod issues;
pub(crate) mod model;
#[cfg(test)]
mod node_tests;
mod personas;
pub(crate) mod place;
pub(crate) mod policy;
mod service;
#[cfg(test)]
mod testing;
pub(crate) mod tools;
pub(crate) mod turn;
pub(crate) mod worker;

/// Marks a task done once its pull request merged. Closing its issue and
/// removing its worktree come with the worker lifecycle.
pub(crate) async fn finish_task(user: &User, task: &TaskRow) -> Result<()> {
    user.set_task_state(task.id, TaskState::Done).await
}
