//! A worker's turns: news from its agent, finished turns, questions, blocks and its end.

use super::super::{
    fleet::{self, Fleet},
    model::{TaskState, WorkerId},
};
use crate::{
    account::user::User,
    store::{ProjectRow, TaskRow, WorkerRow},
};
use anyhow::Result;
use beep_projects::{
    commands::{HostCommand, LABEL_BLOCKED, LABEL_RUNNING},
    issues::run_host,
};
use uuid::Uuid;

/// A worker with what it works on.
pub(super) struct Working {
    pub(super) worker: WorkerRow,
    pub(super) task: TaskRow,
    pub(super) project: ProjectRow,
    pub(super) chat: Uuid,
    /// The worker's machine, as commands name it.
    pub(super) machine: String,
}

pub(super) async fn working(
    fleet: &dyn Fleet,
    user: &User,
    worker: WorkerId,
) -> Result<Option<Working>> {
    let Some(worker) = user.project_worker(worker).await? else {
        return Ok(None);
    };
    let Some(task) = user.project_task(worker.task).await? else {
        return Ok(None);
    };
    let Some(project) = user.project(worker.project).await? else {
        return Ok(None);
    };
    let chat = user.ensure_project_agent(project.id).await?;
    let machines = fleet.machines().await?;
    let machine = fleet::name_of(&machines, &worker.machine);
    Ok(Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }))
}

pub(super) async fn relabel(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
    add: Option<&str>,
    remove: &[&str],
) {
    let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) else {
        return;
    };
    // Someone else's issue keeps its labels.
    if task.issue_foreign {
        return;
    }

    let home = match fleet::home_shell(fleet, user, project).await {
        Ok(home) => home,
        Err(why) => {
            log::info!("projects: an issue label waits: {why}");
            return;
        }
    };
    let command = HostCommand::Relabel {
        number: issue.number,
        add,
        remove,
    };

    if let Err(error) = run_host(home.as_ref(), host, &project.name, command).await {
        log::warn!("projects: unable to relabel an issue: {error:#}");
    }
}

/// Blocks the task if it is still running, labelling its issue.
pub(crate) async fn block(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<()> {
    if task.state == TaskState::Running
        && user
            .move_task_state(task.id, TaskState::Running, TaskState::Blocked)
            .await?
    {
        relabel(
            fleet,
            user,
            project,
            task,
            Some(LABEL_BLOCKED),
            &[LABEL_RUNNING],
        )
        .await;
    }

    Ok(())
}
