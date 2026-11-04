//! A project's pull requests, issues and workflow runs from `crate::repo_watch`'s copy.

use super::{
    Reply, SEND_TIMEOUT,
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
    let open = issues::untracked_now(user, project, read)
        .await
        .map_err(super::projects::issue_refusal)?;
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
