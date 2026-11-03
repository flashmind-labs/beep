//! A project's pull requests, issues and workflow runs from the server's
//! copy (see `crate::repo_watch`), for the project page.

use super::{
    Reply,
    projects::{issue_view, project_of, pull_request_view},
};
use crate::{
    app::App,
    projects::{
        issues,
        model::{IssueHost, ProjectId},
    },
    protocol::*,
    repo_watch::ListsCopy,
    router::Cx,
    store::ProjectRow,
    user::User,
};
use std::sync::Arc;

pub(super) async fn get_project_lists(cx: Cx, req: GetProjectLists) -> Reply<ProjectLists> {
    project_lists(&cx.app, &cx.user, ProjectId(req.project_id), req.refresh).await
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

/// The lists of `id` as `GetProjectLists` gives them.
pub(super) async fn project_lists(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
    refresh: bool,
) -> Reply<ProjectLists> {
    let project = project_of(user, id).await?;
    let copy = app.repo_watch.lists(app, user, &project, refresh).await;
    let issues = untracked(user, &project, &copy).await?;
    Ok(ProjectLists {
        pull_requests: copy.prs.into_iter().map(pull_request_view).collect(),
        prs_error: copy.prs_error,
        issues,
        issues_error: copy.issues_error,
        synced_at: copy.synced_at,
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
    let open = issues::untracked_now(user, project, read)
        .await
        .map_err(super::projects::issue_refusal)?;
    Ok(open.into_iter().map(issue_view).collect())
}

/// The open issues no task tracks, from the copy, for `ListProjectIssues`:
/// the host's error when its last read failed, as before the copy.
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
