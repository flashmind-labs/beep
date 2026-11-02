//! The project agents, their tasks and workers, for the apps. The handlers
//! check the caller and hand the work to functions of the app and the user,
//! which the tests call directly.

use super::Reply;
use crate::{
    app::App,
    projects::{
        fleet::NodeFleet,
        issues::{self, IssueError, IssueWork, ListedPr, OpenIssue},
        lifecycle::fill_slots,
        model::{ProjectId, RepoName, TaskId, TaskState, WorkerAgent, WorkerId},
        tools::{drop_task, reopen_task},
        worker::stop_worker,
    },
    protocol::*,
    router::Cx,
    store::{ProjectRow, TaskRow, WorkerRow},
    user::User,
};
use chrono::{Duration as TimeDelta, Utc};
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};
use strum::VariantNames;
use uuid::Uuid;

/// How long an ended worker still shows.
const ENDED_SHOWN: TimeDelta = TimeDelta::days(1);

pub(super) async fn get_project_agent(cx: Cx, req: GetProjectAgent) -> Reply<ProjectAgent> {
    project_agent(&cx.user, &req.project).await
}

pub(super) async fn list_project_tasks(cx: Cx, req: ListProjectTasks) -> Reply<Vec<ProjectTask>> {
    project_tasks(&cx.user, ProjectId(req.project)).await
}

pub(super) async fn list_project_workers(
    cx: Cx,
    req: ListProjectWorkers,
) -> Reply<Vec<ProjectWorker>> {
    project_workers(&cx.user, ProjectId(req.project)).await
}

pub(super) async fn update_project_task(cx: Cx, req: UpdateProjectTask) -> Reply<ProjectTask> {
    update_task(&cx.app, &cx.user, TaskId(req.task), req.state.into()).await
}

pub(super) async fn stop_project_worker(cx: Cx, req: StopProjectWorker) -> Reply<()> {
    stop(&cx.app, &cx.user, WorkerId(req.worker)).await
}

pub(super) async fn list_projects(cx: Cx, _: ListProjects) -> Reply<Vec<ProjectAgent>> {
    projects(&cx.user).await
}

pub(super) async fn list_project_overview(
    cx: Cx,
    _: ListProjectOverview,
) -> Reply<Vec<ProjectOverview>> {
    project_overview(&cx.user).await
}

// The issue requests are scoped by the project's owner: another user's
// project is not found.

pub(super) async fn list_project_issues(
    cx: Cx,
    req: ListProjectIssues,
) -> Reply<Vec<ProjectIssue>> {
    project_issues(&cx.app, &cx.user, ProjectId(req.project_id)).await
}

pub(super) async fn list_project_pull_requests(
    cx: Cx,
    req: ListProjectPullRequests,
) -> Reply<Vec<ProjectPullRequest>> {
    project_pull_requests(&cx.app, &cx.user, ProjectId(req.project_id)).await
}

pub(super) async fn track_project_issue(cx: Cx, req: TrackProjectIssue) -> Reply<ProjectTask> {
    track_issue(
        &cx.app,
        &cx.user,
        ProjectId(req.project_id),
        req.number,
        req.start.as_ref(),
    )
    .await
}

pub(super) async fn dismiss_project_issue(cx: Cx, req: DismissProjectIssue) -> Reply<()> {
    dismiss_issue(&cx.user, ProjectId(req.project_id), req.number).await
}

pub(super) fn issue_refusal(error: IssueError) -> ApiError {
    match error {
        IssueError::Refused(why) => ApiError::bad_request(why),
        tracked @ IssueError::Tracked(_) => ApiError::Conflict {
            message: tracked.to_string(),
        },
        IssueError::Store(error) => error.into(),
    }
}

fn issue_view(issue: OpenIssue) -> ProjectIssue {
    ProjectIssue {
        number: issue.number,
        title: issue.title,
        labels: issue.labels,
        author: issue.author,
        opened_at: issue.opened_at.into(),
        comments: issue.comments,
        url: issue.url,
    }
}

/// The open issues of the project no task tracks.
async fn project_issues(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
) -> Reply<Vec<ProjectIssue>> {
    let project = project_of(user, id).await?;
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    let open = issues::list_open(&fleet, user, &project)
        .await
        .map_err(issue_refusal)?;
    Ok(open.into_iter().map(issue_view).collect())
}

fn pull_request_view(listed: ListedPr) -> ProjectPullRequest {
    let pr = listed.pr;
    ProjectPullRequest {
        number: pr.number,
        title: pr.title,
        url: pr.url,
        author: pr.author,
        draft: pr.draft,
        mine: listed.mine,
        created_at: pr.created_at.into(),
        updated_at: pr.updated_at.into(),
    }
}

/// Every open pull request of the project, newest first.
async fn project_pull_requests(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
) -> Reply<Vec<ProjectPullRequest>> {
    let project = project_of(user, id).await?;
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    let open = issues::list_open_prs(&fleet, user, &project)
        .await
        .map_err(issue_refusal)?;
    Ok(open.into_iter().map(pull_request_view).collect())
}

/// How the app asked the worker to start, checked.
fn issue_work(start: &IssueStart) -> Result<IssueWork, ApiError> {
    let agent = WorkerAgent::from_str(start.agent.trim()).map_err(|_| {
        ApiError::bad_request(format!(
            "The agent is one of {}.",
            WorkerAgent::VARIANTS.join(", ")
        ))
    })?;
    let machine = start
        .node_id
        .as_deref()
        .map(|id| {
            EndpointId::from_str(id.trim())
                .map_err(|_| ApiError::bad_request("That is not a machine id."))
        })
        .transpose()?;
    Ok(IssueWork { agent, machine })
}

/// Makes a task tracking the issue `number`, then starts it or has it
/// planned.
async fn track_issue(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
    number: u64,
    start: Option<&IssueStart>,
) -> Reply<ProjectTask> {
    let project = project_of(user, id).await?;
    let work = start.map(issue_work).transpose()?;
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    let task = issues::track(&fleet, user, &project, number, work)
        .await
        .map_err(issue_refusal)?;
    Ok(task_view(task))
}

/// Hides the issue `number` from the project's list.
async fn dismiss_issue(user: &User, id: ProjectId, number: u64) -> Reply<()> {
    let project = project_of(user, id).await?;
    if i64::try_from(number).is_err() {
        return Err(ApiError::bad_request("No issue has that number."));
    }
    issues::dismiss(user, &project, number)
        .await
        .map_err(issue_refusal)
}

/// The agent of the project `name`, made with its chat on the first ask.
async fn project_agent(user: &User, name: &str) -> Reply<ProjectAgent> {
    let name = RepoName::parse(name.trim())
        .map_err(|e| ApiError::bad_request(format!("Not a project name: {e}.")))?;
    let project = user.ensure_project(&name).await?;
    agent_of(user, &project).await
}

/// Every project agent. A project whose chat was deleted gets a new one.
async fn projects(user: &User) -> Reply<Vec<ProjectAgent>> {
    let mut agents = Vec::new();
    for project in user.projects().await? {
        agents.push(agent_of(user, &project).await?);
    }
    Ok(agents)
}

async fn project_overview(user: &User) -> Reply<Vec<ProjectOverview>> {
    Ok(user
        .project_overviews()
        .await?
        .into_iter()
        .map(|row| ProjectOverview {
            project_id: row.id.0,
            name: row.name,
            tasks: row.tasks,
            workers: row.workers,
            latest: row.latest.map(task_view),
        })
        .collect())
}

async fn agent_of(user: &User, project: &ProjectRow) -> Reply<ProjectAgent> {
    let chat_id = user.ensure_project_agent(project.id).await?;
    let open = user.project_open_counts(project.id).await?;
    Ok(ProjectAgent {
        project_id: project.id.0,
        name: project.name.to_string(),
        chat_id,
        open_tasks: open.tasks,
        open_workers: open.workers,
    })
}

pub(super) async fn project_of(user: &User, id: ProjectId) -> Reply<ProjectRow> {
    user.project(id).await?.ok_or(ApiError::NotFound {})
}

async fn project_tasks(user: &User, id: ProjectId) -> Reply<Vec<ProjectTask>> {
    let project = project_of(user, id).await?;
    Ok(user
        .project_tasks(project.id)
        .await?
        .into_iter()
        .map(task_view)
        .collect())
}

async fn project_workers(user: &User, id: ProjectId) -> Reply<Vec<ProjectWorker>> {
    let project = project_of(user, id).await?;
    let workers = user
        .project_workers(project.id, Utc::now() - ENDED_SHOWN)
        .await?;
    let names = machine_names(user).await?;
    Ok(workers
        .into_iter()
        .map(|worker| worker_view(worker, &names))
        .collect())
}

/// The names of the user's machines whose keys are still linked, by id.
async fn machine_names(user: &User) -> Reply<HashMap<String, String>> {
    let linked: HashSet<String> = user
        .account_keys()
        .await?
        .into_iter()
        .map(|key| key.endpoint_id)
        .collect();
    Ok(user
        .nodes()
        .await?
        .into_iter()
        .filter(|node| linked.contains(&node.endpoint))
        .map(|node| (node.endpoint, node.name))
        .collect())
}

/// Drops a task, or plans a closed one again.
async fn update_task(
    app: &Arc<App>,
    user: &Arc<User>,
    id: TaskId,
    to: TaskState,
) -> Reply<ProjectTask> {
    let task = user.project_task(id).await?.ok_or(ApiError::NotFound {})?;
    let project = project_of(user, task.project).await?;
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    match to {
        TaskState::Dropped if task.state.is_open() => {
            let said = drop_task(&fleet, user, &project, &task).await?;
            log::info!("projects: the user dropped a task: {said}");
            // The slot its worker held may start a queued task.
            if let Err(error) = fill_slots(&fleet, user).await {
                log::warn!("projects: queued tasks not started: {error:#}");
            }
        }
        TaskState::Planned
            if matches!(
                task.state,
                TaskState::Dropped | TaskState::Blocked | TaskState::Done
            ) =>
        {
            let said = reopen_task(&fleet, user, &project, &task).await?;
            log::info!("projects: the user reopened a task: {said}");
        }
        _ => {
            let from: &'static str = task.state.into();
            let to: &'static str = to.into();
            return Err(ApiError::bad_request(format!(
                "A {from} task cannot become {to}."
            )));
        }
    }
    let task = user.project_task(id).await?.ok_or(ApiError::NotFound {})?;
    Ok(task_view(task))
}

/// Stops a worker; one that already ended stays as it is.
async fn stop(app: &Arc<App>, user: &Arc<User>, id: WorkerId) -> Reply<()> {
    let worker = user
        .project_worker(id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    stop_worker(app, user, &worker).await?;
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    if let Err(error) = fill_slots(&fleet, user).await {
        log::warn!("projects: queued tasks not started: {error:#}");
    }
    Ok(())
}

pub(super) fn task_view(task: TaskRow) -> ProjectTask {
    let (issue_number, issue_url) = match task.issue {
        Some(issue) => (Some(issue.number), Some(issue.url)),
        None => (None, None),
    };
    ProjectTask {
        id: task.id.0,
        slug: task.slug.as_str().to_owned(),
        title: task.title,
        body: task.body,
        state: task.state.into(),
        depends_on: task.depends_on.map(|id| id.0),
        issue_number,
        issue_url,
        branch: task.branch,
        pr_url: task.pr_url,
        created_at: task.created_at.into(),
        updated_at: task.updated_at.into(),
    }
}

fn worker_view(worker: WorkerRow, names: &HashMap<String, String>) -> ProjectWorker {
    let machine_id = worker.machine.to_string();
    let agent: &'static str = worker.agent.into();
    ProjectWorker {
        id: worker.id.0,
        task_id: worker.task.0,
        machine_name: names.get(&machine_id).cloned().unwrap_or_default(),
        machine_id,
        agent: agent.to_owned(),
        session: worker.session,
        folder: worker.folder.as_str().to_owned(),
        state: worker.state.into(),
        last_output: worker.last_output,
        persona: worker.persona,
        started_at: worker.started_at.into(),
        ended_at: worker.ended_at.map(Into::into),
    }
}

/// The cards the user's project chats wait on, each with its chat.
pub(super) async fn project_cards(user: &User) -> Reply<Vec<(Uuid, PendingApproval)>> {
    let chats = user.project_chats().await?;
    Ok(user.open_approvals(&chats).await?)
}

#[cfg(test)]
mod tests;
