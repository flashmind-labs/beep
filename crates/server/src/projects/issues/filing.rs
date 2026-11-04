//! Filing tasks as issues on the project's host.

use super::{
    super::{
        fleet::{self, Fleet},
        model::TaskId,
    },
    MAX_RETRIES,
};
use crate::{
    account::user::User,
    store::{ProjectRow, TaskRow},
};
use anyhow::{Result, anyhow};
use beep_projects::leak::PublicText;
use beep_projects::{
    Shell,
    commands::{HostCommand, parse_created_issue, parse_found_issue},
    issues::run_host,
    model::{IssueHost, IssueRef},
};

/// What marks the issue of `task`, so a retry finds one filed before.
pub(crate) fn marker(task: TaskId) -> String {
    format!("beep-task:{task}")
}

/// The issue body for `task`: its text, then its marker in a comment.
pub(crate) fn issue_body(task: TaskId, body: &str) -> String {
    format!("{}\n\n<!-- {} -->", body.trim_end(), marker(task))
}

/// Files and saves `task`'s issue, reusing one an earlier try filed (found by its marker).
pub(crate) async fn file_issue(
    user: &User,
    shell: &dyn Shell,
    host: &IssueHost,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<IssueRef> {
    let marker = marker(task.id);
    let found = run_host(
        shell,
        host,
        &project.name,
        HostCommand::FindIssue { marker: &marker },
    )
    .await?;
    let issue = match parse_found_issue(&found, &marker)? {
        Some(issue) => issue,
        None => {
            let body = issue_body(task.id, &task.body);
            let out = run_host(
                shell,
                host,
                &project.name,
                HostCommand::CreateIssue {
                    title: &task.title,
                    body: &body,
                },
            )
            .await?;
            parse_created_issue(host, &out)?
        }
    };
    user.set_task_issue(task.id, host, &issue).await?;
    Ok(issue)
}

/// The private details `task`'s issue text would publish.
pub(crate) fn task_leaks(public_text: &PublicText, task: &TaskRow) -> Vec<String> {
    HostCommand::CreateIssue {
        title: &task.title,
        body: &task.body,
    }
    .leaks(public_text)
}

/// Files issues for up to five open tasks missing one. Returns how many it filed.
pub(crate) async fn mirror_missing(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<usize> {
    let Some(host) = &project.roles.issues else {
        return Ok(0);
    };
    let missing: Vec<TaskRow> = user
        .project_tasks(project.id)
        .await?
        .into_iter()
        .filter(|task| task.state.is_open() && task.issue.is_none())
        .take(MAX_RETRIES)
        .collect();
    if missing.is_empty() {
        return Ok(0);
    }
    let home = fleet::home_shell(fleet, user, project)
        .await
        .map_err(|why| anyhow!(why))?;
    let machines = fleet.machines().await?;
    let public_text = fleet::project_public_text(fleet, user, project, &machines).await?;
    let mut filed = 0;
    for task in missing {
        if !task_leaks(&public_text, &task).is_empty() {
            log::warn!("projects: a task's text names private details; no issue filed");
            continue;
        }
        match file_issue(user, home.as_ref(), host, project, &task).await {
            Ok(_) => filed += 1,
            Err(error) => {
                log::warn!("projects: filing an issue failed again: {error:#}");
            }
        }
    }
    Ok(filed)
}
