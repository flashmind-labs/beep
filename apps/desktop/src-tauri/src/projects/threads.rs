//! A mesh project's open issues and pull requests, one read in full, and
//! commenting on one.

use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::projects::{FleetState, named},
    markdown::untrusted_html,
};
use beep_proto::{
    GitItemKind, LinkedPr, ProjectIssue, ProjectPullRequest, PullRequestHead, ThreadDetail,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An open issue no task tracks yet.
#[derive(Debug, Serialize)]
pub struct IssueRow {
    number: u64,
    title: String,
    labels: Vec<String>,
    author: String,
    /// In milliseconds since the Unix epoch.
    opened_at: u64,
    comments: u32,
    url: String,
    /// A pull request is linked to it, to close it.
    linked_pr: bool,
    /// The forge logins it is assigned to.
    assignees: Vec<String>,
}

impl From<ProjectIssue> for IssueRow {
    fn from(i: ProjectIssue) -> Self {
        Self {
            number: i.number,
            title: i.title,
            labels: i.labels,
            author: i.author,
            opened_at: epoch_ms(i.opened_at),
            comments: i.comments,
            url: i.url,
            linked_pr: i.linked_pr,
            assignees: i.assignees,
        }
    }
}

/// `pr` or `issue`.
fn kind_name(kind: GitItemKind) -> &'static str {
    match kind {
        GitItemKind::PullRequest {} => "pr",
        GitItemKind::Issue {} => "issue",
    }
}

/// An open pull request of the project's, anyone's.
#[derive(Debug, Serialize)]
pub struct PullRequestRow {
    number: u64,
    title: String,
    url: String,
    author: String,
    draft: bool,
    mine: bool,
    /// In milliseconds since the Unix epoch.
    created_at: u64,
    labels: Vec<String>,
}

impl PullRequestRow {
    pub(crate) fn labeled(p: ProjectPullRequest, labels: Vec<String>) -> Self {
        Self {
            number: p.number,
            title: p.title,
            url: p.url,
            author: p.author,
            draft: p.draft,
            mine: p.mine,
            created_at: epoch_ms(p.created_at),
            labels,
        }
    }
}

/// The branch pull request `number` is made from, for Work on PR.
#[tauri::command]
pub async fn center_pull_request_head(
    fleet: FleetState<'_>,
    project_id: Uuid,
    number: u64,
) -> Result<PullRequestHead> {
    let (desk, name) = named(&fleet, project_id).await?;
    Ok(desk.pr_head(&name, number).await?)
}

/// A comment or review with its text as cleaned HTML.
#[derive(Debug, Serialize)]
pub struct PostHtmlRow {
    author: String,
    /// In milliseconds since the Unix epoch.
    at: u64,
    html: String,
}

/// A pull request or issue read in full, for the project page's detail
/// pane. Its text is the host's: the page shows it as data.
#[derive(Debug, Serialize)]
pub struct ThreadDetailRow {
    kind: &'static str,
    number: u64,
    title: String,
    url: String,
    author: String,
    /// `open`, `closed` or `merged`.
    state: &'static str,
    draft: bool,
    labels: Vec<String>,
    /// In milliseconds since the Unix epoch.
    opened_at: u64,
    /// The opening post as cleaned HTML.
    body_html: String,
    comments: Vec<PostHtmlRow>,
    earlier: u32,
    /// `none`, `approved` or `changes_requested`.
    review: &'static str,
    /// A `RunState` name, where the host says.
    checks: Option<&'static str>,
    /// For an issue, the pull requests that will close it or mention it.
    linked_prs: Vec<LinkedPr>,
}

impl From<ThreadDetail> for ThreadDetailRow {
    fn from(t: ThreadDetail) -> Self {
        Self {
            kind: kind_name(t.kind),
            number: t.number,
            title: t.title,
            url: t.url,
            author: t.author,
            state: Into::<&'static str>::into(t.state),
            draft: t.draft,
            labels: t.labels,
            opened_at: epoch_ms(t.opened_at),
            body_html: untrusted_html(&t.body),
            comments: t
                .comments
                .into_iter()
                .map(|p| PostHtmlRow {
                    author: p.author,
                    at: epoch_ms(p.at),
                    html: untrusted_html(&p.body),
                })
                .collect(),
            earlier: t.earlier,
            review: Into::<&'static str>::into(t.review),
            checks: t.checks.map(Into::<&'static str>::into),
            linked_prs: t.linked_prs,
        }
    }
}

/// Pull request or issue `number` of the project read in full from its
/// host now, through a machine of the mesh that has the project.
#[tauri::command]
pub async fn center_thread_detail(
    fleet: FleetState<'_>,
    project_id: Uuid,
    number: u64,
) -> Result<Option<ThreadDetailRow>> {
    let (desk, name) = named(&fleet, project_id).await?;
    Ok(Some(desk.thread_detail(&name, number).await?.into()))
}

/// Whether a thread is a pull request or an issue, as the page names it.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    Pr,
    Issue,
}

/// Posts `body` as the user's comment on pull request or issue `number`,
/// through a machine of the mesh that has the project.
#[tauri::command]
pub async fn center_comment_thread(
    fleet: FleetState<'_>,
    project_id: Uuid,
    kind: ThreadKind,
    number: u64,
    body: String,
) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;
    let kind = match kind {
        ThreadKind::Pr => GitItemKind::PullRequest {},
        ThreadKind::Issue => GitItemKind::Issue {},
    };
    Ok(desk.comment_thread(&name, kind, number, &body).await?)
}

#[cfg(test)]
#[path = "../tests/projects_threads.rs"]
mod tests;
