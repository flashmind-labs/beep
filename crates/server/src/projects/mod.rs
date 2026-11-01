#![allow(dead_code, reason = "the project agent uses these in the next tasks")]

//! Projects: a folder of work with its own agent, whose personas live in
//! Markdown files next to the code.

use crate::app::App;
use std::sync::Arc;

pub(crate) use lifecycle::finish_task;
pub(crate) use service::Handled;
pub use service::Projects;

mod commands;
pub(crate) mod fleet;
pub(crate) mod gate;
pub(crate) mod issues;
pub(crate) mod lifecycle;
pub(crate) mod model;
#[cfg(test)]
mod node_tests;
mod personas;
pub(crate) mod place;
pub(crate) mod policy;
mod schedule;
mod service;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod tools;
pub(crate) mod turn;
pub(crate) mod worker;

/// Starts following project workers: fails the ones a restart left
/// starting, arms the pending checks and idle closes, and runs the timer.
pub fn start(app: Arc<App>) {
    app.projects.start_timer(&app);
    let job = service::Job::new(service::JobKind::Startup, |app| async move {
        lifecycle::startup(&app).await
    });
    app.projects.spawn(&app, job);
}
