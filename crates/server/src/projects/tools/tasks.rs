//! `project_tasks`: lists, updates, drops, reopens, merges and comments on tasks.

use super::super::{
    commands::{ChecksState, HostCommand, parse_pr},
    fleet::{self, Fleet},
    gate::{Asks, Gate},
    issues::{self, run_host},
    model::{IssueHost, TaskId, TaskState, pr_number},
};
use super::COMMENT_SLUG;
use super::DROP_SLUG;
use super::MERGE_SLUG;
use super::Planned;
use super::ProjectTools;
use super::REOPEN_SLUG;
use super::TASKS;
use super::UPDATE_SLUG;
use super::card;
use super::field;
use super::leaked;
use crate::{
    account::user::User,
    store::{ProjectRow, TaskRow},
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, sync::Arc};

pub(super) struct TasksTool {
    pub(super) tools: Arc<ProjectTools>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TasksAction {
    List,
    Update,
    Drop,
    Reopen,
    Merge,
    Comment,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TasksArgs {
    action: TasksAction,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    /// A task's slug, or empty to clear.
    #[serde(default)]
    after: Option<String>,
}

#[async_trait]
impl Tool for TasksTool {
    fn name(&self) -> &str {
        TASKS
    }

    fn description(&self) -> &str {
        "The project's tasks. `list` shows them all. `update` changes a task's title, body or \
         the task it waits for (`after`, a slug; empty clears it) and its issue. `drop` stops \
         its worker and closes its issue as not planned; `reopen` plans a dropped or done task \
         again. `merge` squash-merges the task's pull request after the user approves a card \
         showing the PR and its checks; never merge any other way."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "update", "drop", "reopen", "merge", "comment"]},
                "task": {"type": "string", "description": "The task's slug."},
                "title": {"type": "string"},
                "body": {"type": "string", "description": "The new body, or the comment for comment."},
                "after": {"type": "string", "description": "The slug of the task it waits for; empty for none."}
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: TasksArgs = match ctx.parse_args(TASKS) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        let project = self.tools.project().await?;
        let tasks = self.tools.user.project_tasks(project.id).await?;
        if let TasksAction::List = args.action {
            return Ok(ToolResult::success(id, list(&tasks)));
        }
        let Some(slug) = args.task.as_deref() else {
            return Ok(ToolResult::failure(id, "Name the task by its slug."));
        };
        let Some(task) = tasks.iter().find(|task| task.slug.as_str() == slug) else {
            let slugs: Vec<&str> = tasks.iter().map(|task| task.slug.as_str()).collect();
            return Ok(ToolResult::failure(
                id,
                format!("There is no task {slug}. Tasks: {}.", slugs.join(", ")),
            ));
        };
        let done = match args.action {
            TasksAction::List => unreachable!("listed above"),
            TasksAction::Update => self.update(id, &project, &tasks, task, &args).await?,
            TasksAction::Drop => self.drop_task(id, &project, task).await?,
            TasksAction::Reopen => self.reopen(id, &project, task).await?,
            TasksAction::Merge => self.merge(id, &project, task).await?,
            TasksAction::Comment => self.comment(id, &project, task, &args).await?,
        };
        Ok(match done {
            Planned::Done(text) => ToolResult::success(id, text),
            Planned::Refused(text) => ToolResult::failure(id, text),
            Planned::Gated(result) => result,
        })
    }

    fn humanize(&self, args: &Value) -> String {
        match args.get("action").and_then(Value::as_str) {
            Some("merge") => "Merging a pull request".to_owned(),
            Some("comment") => "Commenting on an issue".to_owned(),
            Some("list") => "Reading the tasks".to_owned(),
            _ => "Updating a task".to_owned(),
        }
    }
}

pub(super) fn list(tasks: &[TaskRow]) -> String {
    if tasks.is_empty() {
        return "The project has no tasks yet.".to_owned();
    }
    let mut text = String::new();
    for task in tasks {
        let state: &'static str = task.state.into();
        let _ = write!(text, "- {}: {state}, \"{}\"", task.slug, task.title);
        if let Some(after) = task
            .depends_on
            .and_then(|id| tasks.iter().find(|other| other.id == id))
        {
            let _ = write!(text, ", after {}", after.slug);
        }
        if let Some(issue) = &task.issue {
            let _ = write!(text, ", issue #{} {}", issue.number, issue.url);
        }
        if let Some(pr) = &task.pr_url {
            let _ = write!(text, ", PR {pr}");
        }
        text.push('\n');
    }
    text
}

/// What a gate check turned into, when it did not run.
pub(super) fn gated(gate: Gate) -> Option<Planned> {
    match gate {
        Gate::Run => None,
        Gate::Refuse(reason) => Some(Planned::Refused(reason)),
        Gate::Park(result) => Some(Planned::Gated(result)),
    }
}

/// Whether `task` already waits for `target`, directly or through others.
pub(super) fn waits_on(tasks: &[TaskRow], task: &TaskRow, target: TaskId) -> bool {
    let mut at = Some(task);
    // Each task is passed at most once, so a loop already saved ends too.
    for _ in 0..=tasks.len() {
        let Some(current) = at else {
            return false;
        };
        if current.id == target {
            return true;
        }
        at = current
            .depends_on
            .and_then(|next| tasks.iter().find(|other| other.id == next));
    }
    false
}

impl TasksTool {
    pub(super) async fn update(
        &self,
        id: &str,
        project: &ProjectRow,
        tasks: &[TaskRow],
        task: &TaskRow,
        args: &TasksArgs,
    ) -> Result<Planned> {
        let tools = &self.tools;
        let title = args.title.as_deref().map(str::trim);
        let body = args.body.as_deref().map(str::trim);
        if title.is_none() && body.is_none() && args.after.is_none() {
            return Ok(Planned::Refused(
                "Give a new title, body or after.".to_owned(),
            ));
        }
        if title == Some("") {
            return Ok(Planned::Refused("A task needs a title.".to_owned()));
        }
        let after: Option<Option<TaskId>> = match args.after.as_deref().map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(slug) if slug == task.slug.as_str() => {
                return Ok(Planned::Refused(
                    "A task cannot wait for itself.".to_owned(),
                ));
            }
            Some(slug) => match tasks.iter().find(|other| other.slug.as_str() == slug) {
                Some(other) if waits_on(tasks, other, task.id) => {
                    return Ok(Planned::Refused(format!(
                        "{} cannot wait for {slug}: {slug} already waits for it, so neither \
                         would start.",
                        task.slug
                    )));
                }
                Some(other) => Some(Some(other.id)),
                None => return Ok(Planned::Refused(format!("There is no task {slug}."))),
            },
        };
        let new_title = title.unwrap_or(&task.title);
        let new_body = body.unwrap_or(&task.body);
        let hits = HostCommand::EditIssue {
            number: 0,
            title: new_title,
            body: new_body,
        }
        .leaks(&tools.public_text().await?);
        if !hits.is_empty() {
            return Ok(Planned::Refused(leaked(&hits)));
        }
        let update_card = || {
            let mut fields = vec![field("Task", task.slug.to_string())];
            if let Some(title) = title {
                fields.push(field("Title", title));
            }
            if let Some(body) = body {
                fields.push(field("Body", body));
            }
            if let Some(after) = &args.after {
                fields.push(field(
                    "After",
                    if after.is_empty() { "nothing" } else { after },
                ));
            }
            card("Update task".to_owned(), fields)
        };
        if let Some(stop) = gated(
            tools
                .gate
                .check(id, UPDATE_SLUG, Asks::ByLevel, update_card),
        ) {
            return Ok(stop);
        }
        tools.user.update_task_text(task.id, title, body).await?;
        if let Some(after) = after {
            tools.user.set_task_after(task.id, after).await?;
        }
        let mut text = format!("Updated {}.", task.slug);
        if task.issue_foreign && task.issue.is_some() && (title.is_some() || body.is_some()) {
            text.push_str(" Its issue was opened by someone else, so it stays as it is.");
        } else if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host)
            && (title.is_some() || body.is_some())
        {
            let body = issues::issue_body(task.id, new_body);
            let command = HostCommand::EditIssue {
                number: issue.number,
                title: new_title,
                body: &body,
            };
            if let Err(why) = self.on_home(project, host, command).await? {
                let _ = write!(text, " Its issue was not updated: {why}");
            }
        }
        Ok(Planned::Done(text))
    }

    pub(super) async fn drop_task(
        &self,
        id: &str,
        project: &ProjectRow,
        task: &TaskRow,
    ) -> Result<Planned> {
        let tools = &self.tools;
        if !task.state.is_open() {
            return Ok(Planned::Refused(format!(
                "{} is already closed.",
                task.slug
            )));
        }
        let drop_card = || {
            card(
                "Drop task".to_owned(),
                vec![field("Task", format!("{}: {}", task.slug, task.title))],
            )
        };
        if let Some(stop) = gated(tools.gate.check(id, DROP_SLUG, Asks::ByLevel, drop_card)) {
            return Ok(stop);
        }
        Ok(Planned::Done(
            drop_task(tools.fleet.as_ref(), &tools.user, project, task).await?,
        ))
    }

    pub(super) async fn reopen(
        &self,
        id: &str,
        project: &ProjectRow,
        task: &TaskRow,
    ) -> Result<Planned> {
        let tools = &self.tools;
        if task.state.is_open() {
            return Ok(Planned::Refused(format!("{} is still open.", task.slug)));
        }
        let reopen_card = || {
            card(
                "Reopen task".to_owned(),
                vec![field("Task", format!("{}: {}", task.slug, task.title))],
            )
        };
        if let Some(stop) = gated(
            tools
                .gate
                .check(id, REOPEN_SLUG, Asks::ByLevel, reopen_card),
        ) {
            return Ok(stop);
        }
        Ok(Planned::Done(
            reopen_task(tools.fleet.as_ref(), &tools.user, project, task).await?,
        ))
    }

    pub(super) async fn merge(
        &self,
        id: &str,
        project: &ProjectRow,
        task: &TaskRow,
    ) -> Result<Planned> {
        let tools = &self.tools;
        let Some(host) = &project.roles.prs else {
            return Ok(Planned::Refused(
                "The project opens no pull requests.".to_owned(),
            ));
        };
        let Some((url, number)) = task
            .pr_url
            .as_deref()
            .and_then(|url| Some((url, pr_number(url)?)))
        else {
            return Ok(Planned::Refused(format!(
                "{} has no pull request yet.",
                task.slug
            )));
        };
        let status = self
            .on_home(project, host, HostCommand::PrStatus { number })
            .await?
            .ok()
            .and_then(|out| parse_pr(host, &out).ok().flatten());
        let merge_card = || {
            let checks = match &status {
                None => "unknown".to_owned(),
                Some(pr) => {
                    let checks: &'static str = match pr.checks {
                        ChecksState::None => "none reported",
                        other => other.into(),
                    };
                    checks.to_owned()
                }
            };
            let mut fields = vec![
                field("Task", format!("{}: {}", task.slug, task.title)),
                field("Pull request", url),
                field("Checks", checks),
            ];
            if let Some(review) = status.as_ref().and_then(|pr| pr.review.clone()) {
                fields.push(field("Review", review.to_lowercase().replace('_', " ")));
            }
            card("Merge pull request".to_owned(), fields)
        };
        if let Some(stop) = gated(tools.gate.check(id, MERGE_SLUG, Asks::Always, merge_card)) {
            return Ok(stop);
        }
        if let Err(why) = self
            .on_home(project, host, HostCommand::MergePr { number })
            .await?
        {
            return Ok(Planned::Refused(format!("The merge failed: {why}")));
        }
        super::super::finish_task(tools.fleet.as_ref(), &tools.user, project, task).await?;
        Ok(Planned::Done(format!(
            "Merged {url}; {} is done.",
            task.slug
        )))
    }

    /// Posts one comment on the task's issue, always asking; at most one on someone else's.
    pub(super) async fn comment(
        &self,
        id: &str,
        project: &ProjectRow,
        task: &TaskRow,
        args: &TasksArgs,
    ) -> Result<Planned> {
        let tools = &self.tools;
        let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) else {
            return Ok(Planned::Refused(format!("{} has no issue.", task.slug)));
        };
        let Some(body) = args
            .body
            .as_deref()
            .map(str::trim)
            .filter(|body| !body.is_empty())
        else {
            return Ok(Planned::Refused("Give the comment as body.".to_owned()));
        };
        if task.issue_foreign && task.issue_commented {
            return Ok(Planned::Refused(format!(
                "Beep already commented on issue #{}, which someone else opened; it gets one \
                 comment at most.",
                issue.number
            )));
        }
        let command = HostCommand::CommentIssue {
            number: issue.number,
            body,
        };
        let hits = command.leaks(&tools.public_text().await?);
        if !hits.is_empty() {
            return Ok(Planned::Refused(leaked(&hits)));
        }
        let comment_card = || {
            card(
                "Comment on issue".to_owned(),
                vec![
                    field("Task", format!("{}: {}", task.slug, task.title)),
                    field("Issue", issue.url.clone()),
                    field("Comment", body),
                ],
            )
        };
        if let Some(stop) = gated(
            tools
                .gate
                .check(id, COMMENT_SLUG, Asks::Always, comment_card),
        ) {
            return Ok(stop);
        }
        // Noted first, so two calls cannot both post on someone else's issue.
        if !tools
            .user
            .note_issue_comment(task.id, task.issue_foreign)
            .await?
        {
            return Ok(Planned::Refused(format!(
                "Beep already commented on issue #{}.",
                issue.number
            )));
        }
        Ok(match self.on_home(project, host, command).await? {
            Ok(_) => Planned::Done(format!("Commented on issue #{}.", issue.number)),
            Err(why) => Planned::Refused(format!("The comment failed: {why}")),
        })
    }

    /// Runs `command` on the home machine; the error is for the model.
    pub(super) async fn on_home(
        &self,
        project: &ProjectRow,
        host: &IssueHost,
        command: HostCommand<'_>,
    ) -> Result<Result<String, String>> {
        on_home(
            self.tools.fleet.as_ref(),
            &self.tools.user,
            project,
            host,
            command,
        )
        .await
    }
}

/// Runs `command` on the project's home machine; the error is for the model or user.
pub(super) async fn on_home(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    host: &IssueHost,
    command: HostCommand<'_>,
) -> Result<Result<String, String>> {
    let home = match fleet::home_shell(fleet, user, project).await {
        Ok(home) => home,
        Err(why) => return Ok(Err(why)),
    };
    Ok(run_host(home.as_ref(), host, project, command)
        .await
        .map_err(|error| format!("{error:#}.")))
}

/// Drops an open `task`: stops its workers and closes its issue as not planned.
pub(crate) async fn drop_task(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<String> {
    user.set_task_state(task.id, TaskState::Dropped).await?;
    for worker in user.live_workers().await? {
        if worker.task == task.id {
            super::super::worker::end_worker(fleet, user, &worker).await?;
        }
    }
    // The slot its worker held may start a queued task.
    super::super::lifecycle::refill(fleet, user).await;
    let mut text = format!("Dropped {}.", task.slug);
    if let Some(issue) = task.issue.as_ref().filter(|_| task.issue_foreign) {
        let _ = write!(
            text,
            " Issue #{} was opened by someone else, so it stays open.",
            issue.number
        );
    } else if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) {
        let command = HostCommand::CloseIssue {
            number: issue.number,
            comment: "Dropped: no longer planned.",
            not_planned: true,
        };
        match on_home(fleet, user, project, host, command).await? {
            Ok(_) => {
                let _ = write!(text, " Closed issue #{} as not planned.", issue.number);
            }
            Err(why) => {
                let _ = write!(text, " Its issue is still open: {why}");
            }
        }
    }
    Ok(text)
}

/// Plans a closed `task` again and reopens its issue. Says what happened.
pub(crate) async fn reopen_task(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<String> {
    user.set_task_state(task.id, TaskState::Planned).await?;
    let mut text = format!("Reopened {}; it is planned again.", task.slug);
    if task.issue_foreign {
        // Beep never closed it, so there is nothing to reopen.
    } else if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) {
        let command = HostCommand::ReopenIssue {
            number: issue.number,
        };
        if let Err(why) = on_home(fleet, user, project, host, command).await? {
            let _ = write!(text, " Its issue is still closed: {why}");
        }
    }
    Ok(text)
}
