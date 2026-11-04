//! Due checks: the pull requests of a project's tasks, polled until they settle.

use super::{
    super::{
        commands::{HostCommand, PrState, parse_pr, pr_fingerprint},
        fleet::{self, Fleet},
        issues::run_host,
        model::{CHECK_EVERY, IssueHost, ProjectId, TaskState, pr_number},
        schedule::Due,
        turn::CHECK_LABEL,
    },
    MAX_CHECKED, PR_LIMIT, after, fleet_of, fleet_of_worker, idle_close,
};
use crate::{
    account::user::User,
    app::App,
    projects::finish_task,
    store::{ProjectRow, TaskRow},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::{fmt::Write, sync::Arc};
use tokio::time::timeout;

/// Something on the schedule is due.
pub(crate) async fn on_due(app: &Arc<App>, due: Due, now: DateTime<Utc>) -> Result<()> {
    match due {
        Due::Check(project) => {
            let Some(owner) = app.store.project_owner(project).await? else {
                return Ok(());
            };
            let Some(fleet) = fleet_of(app, &owner) else {
                return Ok(());
            };
            let user = fleet.user.clone();
            check(&fleet, &user, project, now).await
        }
        Due::Idle(worker) => {
            let Some(fleet) = fleet_of_worker(app, worker).await? else {
                return Ok(());
            };
            let user = fleet.user.clone();
            idle_close(&fleet, &user, worker, now).await
        }
        // The projects run a worker's jobs themselves.
        Due::Worker(_) => Ok(()),
    }
}

/// Checks PRs of tasks in review, finishes merged ones, tells the chat, re-arms while needed.
pub(crate) async fn check(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    project: ProjectId,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(project) = user.project(project).await? else {
        fleet.projects().disarm(&Due::Check(project));
        return Ok(());
    };
    // A failure on one pull request must not stop the checks for good.
    let checked = check_prs(fleet, user, &project).await;
    rearm_check(fleet, user, project.id, now).await?;
    checked
}

/// [`check`]'s reading of the project's pull requests.
pub(super) async fn check_prs(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    project: &ProjectRow,
) -> Result<()> {
    let mut changes = Vec::new();
    if let Some(prs) = &project.roles.prs {
        let tasks = user.project_tasks(project.id).await?;
        let review = tasks
            .iter()
            .filter(|task| task.state == TaskState::Review)
            .filter_map(|task| Some((task, pr_number(task.pr_url.as_deref()?)?)))
            .take(MAX_CHECKED);
        for (task, number) in review {
            let pr = match pr_status(fleet, user, project, prs, number).await {
                Ok(Some(pr)) => pr,
                Ok(None) => continue,
                Err(error) => {
                    log::info!("projects: unable to read a pull request: {error:#}");
                    continue;
                }
            };
            let fingerprint = pr_fingerprint(&pr);
            if task.pr_seen.as_deref() != Some(fingerprint.as_str()) {
                user.set_pr_seen(task.id, &fingerprint).await?;
                changes.push(pr_line(task, &pr));
            }
            if pr.merged {
                finish_task(fleet, user, project, task).await?;
            }
        }
    }
    if !changes.is_empty() {
        let chat = user.ensure_project_agent(project.id).await?;
        fleet.post(
            chat,
            CHECK_LABEL,
            format!(
                "Pull requests changed:\n{}\n\nTell the user what needs them, if anything: a \
                 failing check or requested changes go back to the task's worker.",
                changes.join("\n")
            ),
        );
    }
    Ok(())
}

pub(super) async fn rearm_check(
    fleet: &dyn Fleet,
    user: &User,
    project: ProjectId,
    now: DateTime<Utc>,
) -> Result<()> {
    let open = user
        .project_tasks(project)
        .await?
        .iter()
        .any(|task| task.state.wants_checks());
    let due = Due::Check(project);
    if open {
        fleet.projects().arm(due, after(now, CHECK_EVERY));
    } else {
        fleet.projects().disarm(&due);
    }
    Ok(())
}

pub(super) async fn pr_status(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    prs: &IssueHost,
    number: u64,
) -> Result<Option<PrState>> {
    let home = fleet::home_shell(fleet, user, project)
        .await
        .map_err(anyhow::Error::msg)?;
    let command = HostCommand::PrStatus { number };
    let out = timeout(PR_LIMIT, run_host(home.as_ref(), prs, project, command))
        .await
        .context("the host did not answer in time")??;
    parse_pr(prs, &out)
}

pub(super) fn pr_line(task: &TaskRow, pr: &PrState) -> String {
    let state = if pr.merged {
        "merged"
    } else if pr.open {
        "open"
    } else {
        "closed"
    };
    let checks: &'static str = pr.checks.into();
    let mut line = format!("- {}: {} is {state}, checks {checks}", task.slug, pr.url);
    if let Some(review) = &pr.review {
        let _ = write!(line, ", review {}", review.to_lowercase().replace('_', " "));
    }
    line
}
