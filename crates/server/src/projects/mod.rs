//! Projects: a folder of work with its own agent and Markdown personas.

pub mod git_items;
pub mod imports;
pub mod repo_watch;

use crate::app::App;
use std::sync::Arc;

pub(crate) use commands::{CommandOutput, Shell, parse_login, quote};
pub(crate) use lifecycle::finish_task;
pub(crate) use service::Handled;
pub use service::Projects;

pub(crate) mod branches;
mod commands;
pub(crate) mod fleet;
pub(crate) mod gate;
pub(crate) mod issues;
pub(crate) mod lifecycle;
pub(crate) mod model;
#[cfg(test)]
mod node_tests;
mod online_shell;
mod personas;
pub(crate) mod place;
pub(crate) mod policy;
pub(crate) mod review;
mod schedule;
mod service;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod thread_detail;
pub(crate) mod threads;
pub(crate) mod tools;
pub(crate) mod turn;
pub(crate) mod worker;

/// Starts the project lifecycle: startup recovery, the timer and the branch review loop.
pub fn start(app: Arc<App>) {
    app.projects.start_timer(&app);
    let job = service::Job::new(service::JobKind::Startup, |app| async move {
        lifecycle::startup(&app).await
    });
    app.projects.spawn(&app, job);
    tokio::spawn(review::drive::run(Arc::downgrade(&app)));
}
