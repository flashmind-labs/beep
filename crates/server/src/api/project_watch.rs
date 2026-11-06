//! A project's pull requests, issues and workflow runs from `crate::projects::repo_watch`'s copy.

use super::{Reply, SEND_TIMEOUT, projects::project_of};
use crate::projects::stored::StoredProject;
use crate::{
    account::user::User,
    api::router::{Cx, Sink},
    app::App,
    projects::{
        model::ProjectId,
        repo_watch::{Changed, ListsCopy},
    },
    protocol::*,
    store::ProjectRow,
};
use beep_projects::issues::untracked_now;
use beep_projects::issues::{issue_view, pull_request_view};
use beep_projects::model::IssueHost;
use std::sync::Arc;
use tokio::{sync::broadcast::error::RecvError, time::timeout};

pub(super) async fn get_project_lists(cx: Cx, req: GetProjectLists) -> Reply<ProjectLists> {
    project_lists(&cx.app, &cx.user, ProjectId(req.project_id), req.refresh).await
}

pub(super) async fn get_labeled_project_lists(
    cx: Cx,
    req: GetLabeledProjectLists,
) -> Reply<LabeledProjectLists> {
    labeled_lists(&cx.app, &cx.user, ProjectId(req.project_id), req.refresh).await
}

pub(super) async fn list_project_runs(cx: Cx, req: ListProjectRuns) -> Reply<ProjectRuns> {
    let project = project_of(&cx.user, ProjectId(req.project_id)).await?;
    Ok(cx
        .app
        .repo_watch
        .runs(&cx.app, &cx.user, &project, req.refresh)
        .await)
}

pub(super) async fn get_project_run(cx: Cx, req: GetProjectRun) -> Reply<ProjectRunDetail> {
    let project = project_of(&cx.user, ProjectId(req.project_id)).await?;
    cx.app
        .repo_watch
        .run_detail(&cx.app, &cx.user, &project, None, req.run_id)
        .await
        .map_err(ApiError::bad_request)
}

pub(super) async fn list_project_runs_all(cx: Cx, req: ListProjectRunsAll) -> Reply<HostedRuns> {
    let project = project_of(&cx.user, ProjectId(req.project_id)).await?;
    Ok(cx
        .app
        .repo_watch
        .runs_everywhere(&cx.app, &cx.user, &project, req.refresh)
        .await)
}

pub(super) async fn get_project_run_on(cx: Cx, req: GetProjectRunOn) -> Reply<ProjectRunDetail> {
    let project = project_of(&cx.user, ProjectId(req.project_id)).await?;
    let host = IssueHost::of_hostname(req.host.trim())
        .ok_or_else(|| ApiError::bad_request(format!("{} is not a git host.", req.host)))?;
    cx.app
        .repo_watch
        .run_detail(&cx.app, &cx.user, &project, Some(&host), req.run_id)
        .await
        .map_err(ApiError::bad_request)
}

/// Follows a project for its page, sending each time it changes, until the page closes the stream.
pub(super) async fn watch_project(
    cx: Cx,
    req: WatchProject,
    sink: Sink<ProjectChange>,
) -> Reply<()> {
    let id = ProjectId(req.project_id);
    project_of(&cx.user, id).await?;
    let mut changes = cx.app.repo_watch.changes();
    let mut next = Some(change_of(false, false));
    loop {
        if let Some(change) = next.take() {
            match timeout(SEND_TIMEOUT, sink.send(&change)).await {
                Ok(sent) => sent?,
                Err(_) => return Ok(()),
            }
        }
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            changed = changes.recv() => {
                let (lists, runs) = match changed {
                    Ok(Changed { user, project, lists, runs })
                        if user == cx.user.id() && project == id => (lists, runs),
                    Ok(_) => continue,
                    // Some were missed: read everything again.
                    Err(RecvError::Lagged(_)) => (true, true),
                    Err(RecvError::Closed) => return Ok(()),
                };
                next = Some(change_of(lists, runs));
            }
        }
    }
}

/// Why a project is never live: the server reads its host on a timer (see `repo_watch`).
const POLLING_WHY: &str = "Beep reads the host every few minutes while the page is open.";

fn change_of(lists: bool, runs: bool) -> ProjectChange {
    ProjectChange {
        live: false,
        polling_why: Some(POLLING_WHY.to_owned()),
        lists,
        runs,
    }
}

/// The lists of `id` as `GetProjectLists` gives them.
pub(super) async fn project_lists(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
    refresh: bool,
) -> Reply<ProjectLists> {
    Ok(labeled_lists(app, user, id, refresh).await?.lists)
}

/// The lists of `id` as `GetLabeledProjectLists` gives them.
async fn labeled_lists(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
    refresh: bool,
) -> Reply<LabeledProjectLists> {
    let project = project_of(user, id).await?;
    let copy = app.repo_watch.lists(app, user, &project, refresh).await;
    let issues = untracked(user, &project, &copy).await?;
    let pr_labels = copy.prs.iter().map(|pr| pr.pr.labels.clone()).collect();
    Ok(LabeledProjectLists {
        lists: ProjectLists {
            pull_requests: copy.prs.into_iter().map(pull_request_view).collect(),
            prs_error: copy.prs_error,
            issues,
            issues_error: copy.issues_error,
            synced_at: copy.synced_at,
        },
        pr_labels,
    })
}

/// The issues of `copy` no task tracks now.
async fn untracked(
    user: &User,
    project: &ProjectRow,
    copy: &ListsCopy,
) -> Reply<Vec<ProjectIssue>> {
    let Some(read) = &copy.issues else {
        return Ok(Vec::new());
    };
    let open = untracked_now(&StoredProject::new(user, project), read)
        .await
        .map_err(|error| super::projects::issue_refusal(error.into()))?;
    Ok(open.into_iter().map(issue_view).collect())
}

/// The open issues no task tracks, or the host's error when its last read failed.
pub(super) async fn copied_issues(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
) -> Reply<Vec<ProjectIssue>> {
    let lists = project_lists(app, user, id, false).await?;
    match lists.issues_error {
        Some(why) => Err(ApiError::bad_request(why)),
        None => Ok(lists.issues),
    }
}

/// The open pull requests from the copy, for `ListProjectPullRequests`.
pub(super) async fn copied_pull_requests(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
) -> Reply<Vec<ProjectPullRequest>> {
    let lists = project_lists(app, user, id, false).await?;
    match lists.prs_error {
        Some(why) => Err(ApiError::bad_request(why)),
        None => Ok(lists.pull_requests),
    }
}
