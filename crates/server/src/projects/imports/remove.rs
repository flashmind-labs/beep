//! Removing a project from Beep, on request or with a removed machine. Files are never deleted,
//! and what runs on a machine is left to it: the server reaches none.

use crate::{
    account::user::User,
    app::App,
    projects::{
        fleet::RecordedFleet,
        lifecycle::{self, fill_slots},
    },
    store::RemovedProject,
};
use anyhow::Result;
use std::sync::Arc;

/// Removes project `name` and forgets what ran for it; `false` when absent.
pub(crate) async fn remove_project(app: &Arc<App>, user: &Arc<User>, name: &str) -> Result<bool> {
    let Some(removed) = user.remove_project_rows(name).await? else {
        return Ok(false);
    };
    log::info!(
        "project removed; user={} project={name} workers={} terminals={}",
        user.id(),
        removed.workers.len(),
        removed.terminals.len()
    );
    wind_down(app, user, &removed).await;
    Ok(true)
}

/// Winds down after a removed machine: the tasks it left without a worker are blocked.
pub(crate) async fn machine_removed(app: &Arc<App>, user: &Arc<User>, removed: &RemovedProject) {
    let fleet = RecordedFleet::new(app, user);

    for worker in &removed.workers {
        if let Err(e) = lifecycle::machine_removed(&fleet, user, worker).await {
            log::warn!("machine removal: a worker's task not blocked: {e:#}");
        }
    }

    wind_down(app, user, removed).await;
}

/// Forgets what ran for a removed project, each failure logged.
async fn wind_down(app: &Arc<App>, user: &Arc<User>, removed: &RemovedProject) {
    let fleet = RecordedFleet::new(app, user);

    for project in &removed.projects {
        app.projects.forget_project(*project);
    }

    for worker in &removed.workers {
        app.projects.untrack(worker.id);
    }

    if !removed.workers.is_empty()
        && let Err(e) = fill_slots(&fleet, user).await
    {
        log::warn!("project removal: queued tasks not started: {e:#}");
    }
}

#[cfg(test)]
#[path = "../../tests/projects_imports_remove.rs"]
mod tests;
