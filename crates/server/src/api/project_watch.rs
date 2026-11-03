//! A project's pull requests, issues and workflow runs from the server's
//! copy (see `crate::repo_watch`), for the project page.

use super::{
    Reply, SEND_TIMEOUT,
    projects::{issue_view, project_of, pull_request_view},
};
use crate::{
    app::App,
    projects::{issues, model::ProjectId},
    protocol::*,
    repo_watch::ListsCopy,
    repo_watch::{Changed, LiveState},
    router::{Cx, Sink},
    store::ProjectRow,
    user::User,
};
use std::sync::Arc;
use tokio::{sync::broadcast::error::RecvError, time::timeout};

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
        .run_detail(&cx.app, &cx.user, &project, req.run_id)
        .await
        .map_err(ApiError::bad_request)
}

/// Follows a project for its page: whether it is live first, then each
/// time that changes or its copies were read again, until the page closes
/// the stream. While a page follows, one of the user's nodes forwards the
/// repository's webhook (see `crate::repo_watch`).
pub(super) async fn watch_project(
    cx: Cx,
    req: WatchProject,
    sink: Sink<ProjectChange>,
) -> Reply<()> {
    let id = ProjectId(req.project_id);
    project_of(&cx.user, id).await?;
    let mut changes = cx.app.repo_watch.changes();
    let mut following = cx.app.repo_watch.forwards.follow(&cx.app, cx.user.id(), id);
    let mut next = Some(change_of(
        &following.state.borrow_and_update(),
        false,
        false,
    ));
    loop {
        if let Some(change) = next.take() {
            match timeout(SEND_TIMEOUT, sink.send(&change)).await {
                Ok(sent) => sent?,
                Err(_) => return Ok(()),
            }
        }
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            changed = following.state.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                next = Some(change_of(&following.state.borrow_and_update(), false, false));
            }
            changed = changes.recv() => {
                let (lists, runs) = match changed {
                    Ok(Changed { user, project, lists, runs })
                        if user == cx.user.id() && project == id => (lists, runs),
                    Ok(_) => continue,
                    // Some were missed: read everything again.
                    Err(RecvError::Lagged(_)) => (true, true),
                    Err(RecvError::Closed) => return Ok(()),
                };
                next = Some(change_of(&following.state.borrow(), lists, runs));
            }
        }
    }
}

fn change_of(state: &LiveState, lists: bool, runs: bool) -> ProjectChange {
    ProjectChange {
        live: state.live,
        polling_why: state.why.clone(),
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
