//! Mirroring tasks to issues on the project's issues host.

use super::{
    commands::{HostCommand, Shell, parse_created_issue},
    fleet::{self, Fleet},
    model::{IssueHost, IssueRef},
};
use crate::{
    leak::PublicText,
    store::{ProjectRow, TaskRow},
    user::User,
};
use anyhow::{Result, anyhow, bail};
use std::time::Duration;

/// Most issues retried for tasks without one, per wake.
const MAX_RETRIES: usize = 5;
/// How long one host command may take.
pub(crate) const HOST_LIMIT: Duration = Duration::from_secs(30);

/// Runs `command` against the project's `host`, failing unless it exits 0.
pub(crate) async fn run_host(
    shell: &dyn Shell,
    host: &IssueHost,
    project: &ProjectRow,
    command: HostCommand<'_>,
) -> Result<String> {
    let rendered = command.render(host, &project.name);
    let out = shell.run(&rendered, HOST_LIMIT).await?;
    match out.code {
        Some(0) => Ok(out.out),
        code => {
            let tail: String = out
                .out
                .chars()
                .rev()
                .take(400)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            bail!(
                "the host command exited with {}: {}",
                code.map_or("no code".to_owned(), |code| code.to_string()),
                tail.trim()
            )
        }
    }
}

/// Files the issue for `task` and saves it on the task.
pub(crate) async fn file_issue(
    user: &User,
    shell: &dyn Shell,
    host: &IssueHost,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<IssueRef> {
    let out = run_host(
        shell,
        host,
        project,
        HostCommand::CreateIssue {
            title: &task.title,
            body: &task.body,
        },
    )
    .await?;
    let issue = parse_created_issue(host, &out)?;
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

/// Files issues for up to five open tasks that have none, as when an
/// earlier try failed. Returns how many it filed.
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
    let public_text = fleet::public_text(&fleet.machines().await?);
    let mut filed = 0;
    for task in missing {
        if !task_leaks(&public_text, &task).is_empty() {
            log::warn!("projects: a task's text names private details; no issue filed");
            continue;
        }
        match file_issue(user, home.shell.as_ref(), host, project, &task).await {
            Ok(_) => filed += 1,
            Err(error) => {
                log::warn!("projects: filing an issue failed again: {error:#}");
            }
        }
    }
    Ok(filed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        projects::{
            model::{MachinePath, RepoName, Roles, Visibility},
            testing::FakeFleet,
        },
        store::{CheckoutRow, NewTask, TestDb},
    };

    fn task(title: &str) -> NewTask {
        NewTask {
            title: title.into(),
            body: "Make it pass.".into(),
            depends_on: None,
            needs: Vec::new(),
        }
    }

    #[tokio::test]
    async fn missing_issues_are_filed_at_most_five_a_wake() {
        let db = TestDb::new().await.unwrap();
        let user = User::for_test(db.store().await.unwrap(), "alice");
        let mut fleet = FakeFleet::default();
        let (home, shell) = fleet.add(1, "build-box", &[], "/srv");
        shell.answer(
            "issue create",
            0,
            "https://github.com/acme/widget/issues/3\n",
        );
        let project = user
            .ensure_project(&RepoName::parse("acme/widget").unwrap())
            .await
            .unwrap();
        user.add_checkout(
            project.id,
            &CheckoutRow {
                machine: home,
                path: MachinePath::parse("/srv/widget").unwrap(),
                home: true,
            },
        )
        .await
        .unwrap();
        let roles = Roles {
            issues: Some(IssueHost::Github),
            prs: None,
        };
        user.set_project_setup(project.id, &roles, Visibility::Private, "main")
            .await
            .unwrap();
        let project = user.project(project.id).await.unwrap().unwrap();
        let tasks: Vec<NewTask> = (1..=6).map(|n| task(&format!("Task {n}"))).collect();
        user.create_tasks(project.id, &tasks).await.unwrap();
        assert_eq!(mirror_missing(&fleet, &user, &project).await.unwrap(), 5);
        assert_eq!(mirror_missing(&fleet, &user, &project).await.unwrap(), 1);
        assert_eq!(mirror_missing(&fleet, &user, &project).await.unwrap(), 0);
        assert_eq!(shell.ran_with("issue create").len(), 6);
    }
}
