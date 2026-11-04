//! Starting, reattaching, closing and finishing workers, and filling free slots.

use super::{
    super::{
        fleet::{self, Fleet, NodeFleet},
        model::{IDLE_CLOSE, TaskState, WorkerId, WorkerState},
        schedule::Due,
        turn::{REPLAN_LABEL, WORKER_LABEL},
        worker::Started,
    },
    MAX_FILLS, REATTACH_AT_ONCE, Working, after, block, catch_up, working,
};
use crate::{
    account::user::User,
    api::transport::READ_TIMEOUT,
    app::App,
    nodes::SessionRefused,
    protocol::node::agent::Status,
    store::{ProjectRow, StartRefused, TaskRow, WorkerRow},
};
use anyhow::Result;
use beep_projects::{
    commands::{HostCommand, worktree_remove},
    issues::run_host,
};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use std::sync::Arc;
use tokio::time::timeout;

/// `machine` reconnected: re-attaches its live workers, failing those whose session is gone.
pub(crate) async fn reconnected(
    app: &Arc<App>,
    user: &Arc<User>,
    machine: iroh::EndpointId,
) -> Result<()> {
    let fleet = NodeFleet::new(app, user);
    reattach_all(&fleet, user, machine).await
}

pub(crate) async fn reattach_all(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    machine: iroh::EndpointId,
) -> Result<()> {
    let workers = user.live_workers_on(&machine).await?;
    futures::stream::iter(workers)
        .for_each_concurrent(REATTACH_AT_ONCE, |worker| async move {
            if let Err(error) = reattach(fleet, user, &worker).await {
                log::warn!("projects: unable to re-attach a worker: {error:#}");
            }
        })
        .await;
    Ok(())
}

pub(super) async fn reattach(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: &WorkerRow,
) -> Result<()> {
    // One still starting gets its session from the start under way.
    let Some(session) = &worker.session else {
        return Ok(());
    };
    let chat = user.ensure_project_agent(worker.project).await?;
    // Tracked first, so what it reports from now on reaches the project.
    fleet.projects().track(worker.machine, session, worker.id);
    match timeout(READ_TIMEOUT, fleet.attach(worker, chat)).await {
        Ok(Ok(info)) => catch_up(fleet, user, worker.id, info).await,
        Ok(Err(why)) if why.downcast_ref::<SessionRefused>().is_some() => {
            log::info!("projects: a worker's session is gone: {why:#}");
            gone(fleet, user, worker.id).await
        }
        // A stream that would not open or closed early, as a machine that
        // flaps does: the worker stays as it was and the next reconnect tries
        // again.
        Ok(Err(why)) => {
            log::warn!("projects: unable to re-attach a worker; it stays as it was: {why:#}");
            Ok(())
        }
        Err(_) => {
            log::warn!("projects: the machine did not answer for a worker; it stays as it was");
            Ok(())
        }
    }
}

/// The machine no longer has the worker's session.
pub(super) async fn gone(fleet: &dyn Fleet, user: &Arc<User>, worker: WorkerId) -> Result<()> {
    let Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }) = working(fleet, user, worker).await?
    else {
        return Ok(());
    };
    fleet.projects().untrack(worker.id);
    if !worker.state.is_live() {
        return Ok(());
    }
    user.set_worker_state(worker.id, WorkerState::Failed)
        .await?;
    block(fleet, user, &project, &task).await?;
    fleet.post(
        chat,
        REPLAN_LABEL,
        format!(
            "The worker for {} on {machine} is gone; the machine no longer has its session.",
            task.slug
        ),
    );
    Ok(())
}

/// `worker`'s machine was removed, its row with it; nothing to do when its project went too.
pub(crate) async fn machine_removed(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: &WorkerRow,
) -> Result<()> {
    let (Some(task), Some(project)) = (
        user.project_task(worker.task).await?,
        user.project(worker.project).await?,
    ) else {
        return Ok(());
    };
    block(fleet, user, &project, &task).await?;
    let chat = user.ensure_project_agent(project.id).await?;
    fleet.post(
        chat,
        REPLAN_LABEL,
        format!(
            "The worker for {} is gone; its machine was removed.",
            task.slug
        ),
    );
    Ok(())
}

/// Closes `worker` if still idle at `now`: waiting, not asking, last turn [`IDLE_CLOSE`] ago.
pub(crate) async fn idle_close(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: WorkerId,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }) = working(fleet, user, worker).await?
    else {
        return Ok(());
    };
    // Working again, or asking: a later turn arms it again.
    if worker.state != WorkerState::Waiting || worker.asking {
        return Ok(());
    }
    let Some(last) = worker.last_turn_at else {
        return Ok(());
    };
    let due = after(last, IDLE_CLOSE);
    if due > now {
        fleet.projects().arm(Due::Idle(worker.id), due);
        return Ok(());
    }
    // A turn steered outside project_worker leaves the worker waiting while
    // its session works: the session says whether it is idle.
    match fleet.session_info(&worker).await {
        Ok(info)
            if matches!(
                info.state.status,
                Status::Starting | Status::Working | Status::Asking
            ) =>
        {
            fleet
                .projects()
                .arm(Due::Idle(worker.id), after(now, IDLE_CLOSE));
            return Ok(());
        }
        Ok(_) => {}
        Err(error) => log::info!("projects: closing an idle worker unseen: {error:#}"),
    }
    close(fleet, user, &worker).await?;
    block(fleet, user, &project, &task).await?;
    let stays = if task.state == TaskState::Running {
        "Its task is blocked until it starts again."
    } else {
        "Its task stays as it was."
    };
    fleet.post(
        chat,
        WORKER_LABEL,
        format!(
            "Closed the worker for {} on {machine}: it was idle for {} minutes with nothing \
             asked. {stays}",
            task.slug,
            IDLE_CLOSE.as_secs() / 60
        ),
    );
    fill_slots(fleet, user).await
}

/// Ends `worker` and closes its session, untracked first so its end is not taken as news.
pub(super) async fn close(fleet: &dyn Fleet, user: &User, worker: &WorkerRow) -> Result<()> {
    fleet.projects().untrack(worker.id);
    if let Some(session) = &worker.session
        && let Err(error) = fleet
            .close_session(&worker.machine, worker.agent, session)
            .await
    {
        log::warn!("projects: unable to close a worker's session: {error:#}");
    }
    user.set_worker_state(worker.id, WorkerState::Ended).await?;
    Ok(())
}

/// The task is done: closes its issue, ends its worker, removes its worktree, frees the slot.
pub(crate) async fn finish_task(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<()> {
    user.set_task_state(task.id, TaskState::Done).await?;
    if let Some(issue) = &task.issue
        && task.issue_foreign
        && task.state != TaskState::Done
    {
        // Someone else's issue stays open; one comment may go, after asking.
        let chat = user.ensure_project_agent(project.id).await?;
        let mut text = format!(
            "{} is done. Its issue #{} was opened by someone else, so Beep leaves it open.",
            task.slug, issue.number
        );
        if !task.issue_commented {
            text.push_str(
                " If a note there helps, offer the user one comment with project_tasks comment.",
            );
        }
        fleet.post(chat, WORKER_LABEL, text);
    } else if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host)
        && task.state != TaskState::Done
    {
        let comment = match &task.pr_url {
            Some(url) => format!("Done in {url}"),
            None => "Done.".to_owned(),
        };
        let command = HostCommand::CloseIssue {
            number: issue.number,
            comment: &comment,
            not_planned: false,
        };
        match fleet::home_shell(fleet, user, project).await {
            Ok(home) => {
                if let Err(error) = run_host(home.as_ref(), host, &project.name, command).await {
                    log::warn!("projects: unable to close a done task's issue: {error:#}");
                }
            }
            Err(why) => log::info!("projects: a done task's issue stays open: {why}"),
        }
    }
    let worker = user
        .project_workers(project.id, task.created_at)
        .await?
        .into_iter()
        .rfind(|worker| worker.task == task.id);
    if let Some(worker) = worker {
        if worker.state.is_live() {
            close(fleet, user, &worker).await?;
        }
        remove_worktree(fleet, user, project, task, &worker).await?;
    }
    fill_slots(fleet, user).await
}

pub(super) async fn remove_worktree(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
    worker: &WorkerRow,
) -> Result<()> {
    let checkout = user
        .checkouts(project.id)
        .await?
        .into_iter()
        .find(|checkout| {
            checkout.machine == worker.machine
                && task.slug.worktree(&checkout.path) == worker.folder
        });
    let Some(checkout) = checkout else {
        return Ok(());
    };
    match fleet.shell(&worker.machine) {
        Some(shell) => {
            let out = shell
                .run(
                    &worktree_remove(&checkout.path, &task.slug),
                    beep_projects::issues::HOST_LIMIT,
                )
                .await?;
            if out.exit_code != Some(0) {
                log::warn!(
                    "projects: unable to remove a done task's worktree: {}",
                    out.output
                );
            }
        }
        None => {
            let machines = fleet.machines().await?;
            let chat = user.ensure_project_agent(project.id).await?;
            fleet.post(
                chat,
                WORKER_LABEL,
                format!(
                    "{} is done. Its worktree on {} stays until that machine is back online.",
                    task.slug,
                    fleet::name_of(&machines, &worker.machine)
                ),
            );
        }
    }
    Ok(())
}

/// [`fill_slots`] after a slot freed; a failure is only logged.
pub(crate) async fn refill(fleet: &dyn Fleet, user: &Arc<User>) {
    if let Err(error) = fill_slots(fleet, user).await {
        log::warn!("projects: queued tasks not started: {error:#}");
    }
}

/// Starts the user's queued tasks, oldest first, until the slots are full.
pub(crate) async fn fill_slots(fleet: &dyn Fleet, user: &Arc<User>) -> Result<()> {
    let full = StartRefused::UserFull.to_string();
    for task in user.queued_tasks().await?.into_iter().take(MAX_FILLS) {
        let Some(request) = task.start.clone() else {
            continue;
        };
        let Some(project) = user.project(task.project).await? else {
            continue;
        };
        match fleet.start_task(&project, &task, &request).await {
            Ok(Started::Running(worker)) => {
                let chat = user.ensure_project_agent(project.id).await?;
                fleet.post(
                    chat,
                    WORKER_LABEL,
                    format!(
                        "A slot freed, so {} started with {} in {}.",
                        task.slug, worker.agent, worker.folder
                    ),
                );
            }
            Ok(Started::Queued(why)) if why == full => break,
            Ok(Started::Queued(_)) => {}
            Err(error) => {
                log::info!("projects: a queued task did not start: {error}");
                start_failed(fleet, user, &project, &task, &error.to_string()).await?;
            }
        }
    }
    Ok(())
}

/// A start of `task` failed: drops its start request and tells the project chat once.
pub(crate) async fn start_failed(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
    why: &str,
) -> Result<()> {
    user.set_task_start(task.id, None).await?;
    let chat = user.ensure_project_agent(project.id).await?;
    fleet.post(
        chat,
        REPLAN_LABEL,
        format!(
            "{} could not start: {why} It will not be tried again on its own.",
            task.slug
        ),
    );
    Ok(())
}
