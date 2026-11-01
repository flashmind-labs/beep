//! Mirroring tasks to issues on the project's issues host.

use super::{
    commands::{HostCommand, Shell, parse_created_issue, parse_found_issue},
    fleet::{self, Fleet},
    model::{IssueHost, IssueRef, TaskId},
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

/// What marks the issue of `task`, so a retry finds one filed before.
pub(crate) fn marker(task: TaskId) -> String {
    format!("beep-task:{task}")
}

/// The issue body for `task`: its text, then its marker in a comment.
pub(crate) fn issue_body(task: TaskId, body: &str) -> String {
    format!("{}\n\n<!-- {} -->", body.trim_end(), marker(task))
}

/// Files the issue for `task` and saves it on the task. An issue an earlier
/// try filed without hearing back is found by its marker and kept.
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
        project,
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
                project,
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
    let machines = fleet.machines().await?;
    let public_text = fleet::project_public_text(fleet, user, project, &machines).await?;
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
            start: None,
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

    #[tokio::test]
    async fn an_issue_already_filed_is_found_not_filed_again() {
        let db = TestDb::new().await.unwrap();
        let user = User::for_test(db.store().await.unwrap(), "alice");
        let mut fleet = FakeFleet::default();
        let (home, shell) = fleet.add(1, "build-box", &[], "/srv");
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
        let made = user
            .create_tasks(project.id, &[task("Found"), task("New")])
            .await
            .unwrap();
        // An earlier try filed the first task's issue but never heard back.
        let found = serde_json::json!([{
            "number": 7,
            "url": "https://github.com/acme/widget/issues/7",
            "body": format!("Make it pass.\n\n<!-- beep-task:{} -->", made[0].id),
        }]);
        shell.answer(&format!("beep-task:{}", made[0].id), 0, &found.to_string());
        shell.answer(
            "issue create",
            0,
            "https://github.com/acme/widget/issues/8\n",
        );
        assert_eq!(mirror_missing(&fleet, &user, &project).await.unwrap(), 2);
        let tasks = user.project_tasks(project.id).await.unwrap();
        let numbers: Vec<Option<u64>> = tasks
            .iter()
            .map(|task| task.issue.as_ref().map(|issue| issue.number))
            .collect();
        assert_eq!(numbers, [Some(7), Some(8)]);
        let created = shell.ran_with("issue create");
        assert_eq!(created.len(), 1, "{created:?}");
        // The new issue carries its task's marker, so a retry finds it.
        assert!(
            created[0].contains(&format!("<!-- beep-task:{} -->", made[1].id)),
            "{}",
            created[0]
        );
        assert_eq!(shell.ran_with("issue list").len(), 2);
    }
}
