#![allow(dead_code, reason = "the project agent uses these in the next tasks")]

//! Projects: a folder of work with its own agent, whose personas live in
//! Markdown files next to the code.

use crate::{
    store::{TaskRow, WorkerRow},
    user::User,
};
use anyhow::Result;
use model::{TaskState, WorkerState};

pub use service::Projects;

mod commands;
pub(crate) mod fleet;
pub(crate) mod gate;
pub(crate) mod issues;
pub(crate) mod model;
mod personas;
pub(crate) mod place;
pub(crate) mod policy;
mod service;
#[cfg(test)]
mod testing;
pub(crate) mod tools;
pub(crate) mod turn;

/// Stops a task's worker. For now it only marks the worker ended; closing
/// its session on the node comes with the worker tool.
pub(crate) async fn stop_worker(user: &User, worker: &WorkerRow) -> Result<()> {
    user.set_worker_state(worker.id, WorkerState::Ended).await?;
    Ok(())
}

/// Marks a task done once its pull request merged. Closing its issue and
/// removing its worktree come with the worker lifecycle.
pub(crate) async fn finish_task(user: &User, task: &TaskRow) -> Result<()> {
    user.set_task_state(task.id, TaskState::Done).await
}
