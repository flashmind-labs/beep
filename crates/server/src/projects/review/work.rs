//! What a review asks of the project: the branch's PR and a task to fix open findings.

use super::super::{
    fleet::{Fleet, NodeFleet},
    lifecycle::start_failed,
    model::{StartRequest, WorkerAgent},
    turn::WORKER_LABEL,
    worker::Started,
};
use crate::projects::stored::StoredProject;
use crate::{
    account::user::User,
    app::App,
    nodes::Online,
    protocol::ReviewChecks,
    store::{NewTask, PrRow, ProjectRow, TaskRow},
};
use anyhow::{Context, Result};
use beep_projects::issues::issue_host;
use beep_projects::{
    commands::{ChecksState, HostCommand, parse_pr},
    issues::run_host,
};
use std::sync::Arc;

fn checks(state: ChecksState) -> ReviewChecks {
    match state {
        ChecksState::Pending => ReviewChecks::Pending {},
        ChecksState::Passing => ReviewChecks::Passing {},
        ChecksState::Failing => ReviewChecks::Failing {},
        ChecksState::None => ReviewChecks::None {},
    }
}

/// The open PR of `branch` into `base`, via the host CLI on `machine`; `None` if no host or PR.
pub(crate) async fn find_pr(
    machine: &Online,
    user: &User,
    project: &ProjectRow,
    branch: &str,
    base: &str,
) -> Result<Option<PrRow>> {
    let host = match &project.roles.prs {
        Some(host) => host.clone(),
        None => match issue_host(&StoredProject::new(user, project)).await? {
            Some(host) => host,
            None => return Ok(None),
        },
    };
    let out = run_host(
        machine,
        &host,
        &project.name,
        HostCommand::FindPr { branch, base },
    )
    .await?;
    Ok(parse_pr(&host, &out)?.filter(|pr| pr.open).map(|pr| PrRow {
        number: pr.number,
        url: pr.url,
        checks: checks(pr.checks),
    }))
}

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
    let fleet = NodeFleet::new(app, user);
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
    let chat = user.ensure_project_agent(project.id).await?;
    match fleet.start_task(project, &task, &request).await {
        Ok(Started::Running(worker)) => fleet.post(
            chat,
            WORKER_LABEL,
            format!(
                "The user started {} to fix review findings on {} with {} in {}.",
                task.slug, fix.branch, worker.agent, worker.folder
            ),
        ),
        // It starts when a slot frees, and that start is reported.
        Ok(Started::Queued(_)) => {}
        Err(error) => {
            log::info!("projects: a review fix did not start: {error}");
            start_failed(&fleet, user, project, &task, &error.to_string()).await?;
        }
    }
    Ok(user.project_task(task.id).await?.unwrap_or(task))
}
