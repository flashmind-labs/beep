//! What a review asks of the project: a task to fix open findings.

use super::super::{
    fleet::{Fleet, RecordedFleet},
    lifecycle::start_failed,
    model::{StartRequest, WorkerAgent},
    worker::Started,
};
use crate::{
    account::user::User,
    app::App,
    store::{NewTask, ProjectRow, TaskRow},
};
use anyhow::{Context, Result};
use std::sync::Arc;

/// A task fixing a review's findings, and where its worker runs.
#[derive(Debug, Clone)]
pub(crate) struct Fix {
    pub(crate) title: String,
    pub(crate) body: String,
    /// The branch its worktree starts from.
    pub(crate) branch: String,
    pub(crate) agent: Option<WorkerAgent>,
    /// The machine, by its command name.
    pub(crate) machine: Option<String>,
}

/// Makes the fix task and starts its worker; a failed start is reported and the task stays planned.
pub(crate) async fn start_fix(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    fix: Fix,
) -> Result<TaskRow> {
    let fleet = RecordedFleet::new(app, user);
    let request = StartRequest {
        machine: fix.machine,
        agent: fix.agent,
        persona: None,
        inputs: Vec::new(),
        base: Some(fix.branch.clone()),
    };
    let new = NewTask {
        title: fix.title,
        body: fix.body,
        depends_on: None,
        needs: Vec::new(),
        start: Some(request.clone()),
    };
    let task = user
        .create_tasks(project.id, &[new])
        .await?
        .into_iter()
        .next()
        .context("the task was not made")?;
    match fleet.start_task(project, &task, &request).await {
        // It starts when a slot frees, and that start is reported.
        Ok(Started::Queued(_)) => {}
        Err(error) => {
            log::info!("projects: a review fix did not start: {error}");
            start_failed(&fleet, user, project, &task, &error.to_string()).await?;
        }
    }

    Ok(user.project_task(task.id).await?.unwrap_or(task))
}
