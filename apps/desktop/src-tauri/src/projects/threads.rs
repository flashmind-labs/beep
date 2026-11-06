//! A project's open issues and the user's pull requests and issues there:
//! starting work on an issue or hiding it, asking the agent for a reply,
//! and the chats about one thread.

use crate::{
    beep::{Service, epoch_ms},
    chat::markdown::untrusted_html,
    error::{DesktopError, Result},
    fleet::projects::{FleetState, mesh_project},
    projects::{known, needs_update},
};
use beep_proto::{
    GetProjectThread, GetPullRequestHead, GetThreadDetail, GitItemKind, IssueStart,
    ListProjectIssues, ListProjectPullRequests, ListProjectThreads, ListThreadChats,
    OpenThreadChat, ProjectIssue, ProjectPullRequest, ProjectThread, PullRequestHead,
    ReplyToThread, ThreadChat, ThreadDetail, ThreadPost, ThreadView, TrackProjectIssue,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn outdated(e: DesktopError) -> DesktopError {
    needs_update(e, "for this")
}

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

/// One of the user's open pull requests or issues.
#[derive(Debug, Serialize)]
pub struct ThreadRow {
    kind: &'static str,
    repo: String,
    number: u64,
    title: String,
    url: String,
    last_other: String,
    needs_reply: bool,
    /// In milliseconds since the Unix epoch.
    updated_at: u64,
}

impl From<ProjectThread> for ThreadRow {
    fn from(t: ProjectThread) -> Self {
        Self {
            kind: kind_name(t.kind),
            repo: t.repo,
            number: t.number,
            title: t.title,
            url: t.url,
            last_other: t.last_other,
            needs_reply: t.needs_reply,
            updated_at: epoch_ms(t.updated_at),
        }
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
    /// In milliseconds since the Unix epoch.
    updated_at: u64,
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
            updated_at: epoch_ms(p.updated_at),
            labels,
        }
    }
}

impl From<ProjectPullRequest> for PullRequestRow {
    fn from(p: ProjectPullRequest) -> Self {
        Self::labeled(p, Vec::new())
    }
}

/// A chat about one thread.
#[derive(Debug, Serialize)]
pub struct ThreadChatRow {
    chat_id: Uuid,
    kind: &'static str,
    repo: String,
    number: u64,
    title: String,
}

impl From<ThreadChat> for ThreadChatRow {
    fn from(c: ThreadChat) -> Self {
        Self {
            chat_id: c.chat_id,
            kind: kind_name(c.kind),
            repo: c.repo,
            number: c.number,
            title: c.title,
        }
    }
}

/// A comment or review on a thread, for the card atop its chat.
#[derive(Debug, Serialize)]
pub struct ThreadPostRow {
    author: String,
    /// In milliseconds since the Unix epoch.
    at: u64,
    body: String,
}

impl From<ThreadPost> for ThreadPostRow {
    fn from(p: ThreadPost) -> Self {
        Self {
            author: p.author,
            at: epoch_ms(p.at),
            body: p.body,
        }
    }
}

/// A pull request or issue read from its host now, for the card atop the
/// chat about it.
#[derive(Debug, Serialize)]
pub struct ThreadCardRow {
    kind: &'static str,
    number: u64,
    title: String,
    url: String,
    author: String,
    body: String,
    needs_reply: bool,
    comments: Vec<ThreadPostRow>,
    earlier: u32,
}

impl From<ThreadView> for ThreadCardRow {
    fn from(t: ThreadView) -> Self {
        Self {
            kind: kind_name(t.kind),
            number: t.number,
            title: t.title,
            url: t.url,
            author: t.author,
            body: t.body,
            needs_reply: t.needs_reply,
            comments: t.comments.into_iter().map(Into::into).collect(),
            earlier: t.earlier,
        }
    }
}

/// The project's open issues no task tracks; `None` from a server too old
/// to list them.
#[tauri::command]
pub async fn center_project_issues(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project_id: Uuid,
) -> Result<Option<Vec<IssueRow>>> {
    if let Some((desk, name)) = mesh_project(&fleet, project_id).await? {
        let issues = desk.issues(&name).await?;
        return Ok(Some(issues.into_iter().map(Into::into).collect()));
    }
    Ok(known(beep.call(ListProjectIssues { project_id }).await)?
        .map(|rows| rows.into_iter().map(Into::into).collect()))
}

/// Makes a task of issue `number`: with `agent`, a worker starts on it on
/// `node_id` (or a machine picked at start); without, the project agent
/// plans it.
#[tauri::command]
pub async fn center_track_issue(
    beep: Service<'_>,
    project_id: Uuid,
    number: u64,
    agent: Option<String>,
    node_id: Option<String>,
) -> Result<Uuid> {
    let start = agent.map(|agent| IssueStart { agent, node_id });
    let task = beep
        .call(TrackProjectIssue {
            project_id,
            number,
            start,
        })
        .await
        .map_err(outdated)?;
    Ok(task.id)
}

/// The user's open pull requests and issues in the project; `None` with no
/// server or one too old to list them.
#[tauri::command]
pub async fn center_project_threads(
    beep: Service<'_>,
    project_id: Uuid,
) -> Result<Option<Vec<ThreadRow>>> {
    if !beep.has_server() {
        return Ok(None);
    }
    Ok(known(beep.call(ListProjectThreads { project_id }).await)?
        .map(|rows| rows.into_iter().map(Into::into).collect()))
}

/// Every open pull request of the project, newest first; `None` from a
/// server too old to list them.
#[tauri::command]
pub async fn center_project_pull_requests(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project_id: Uuid,
) -> Result<Option<Vec<PullRequestRow>>> {
    if let Some((desk, name)) = mesh_project(&fleet, project_id).await? {
        let prs = desk.pull_requests(&name).await?;
        return Ok(Some(prs.into_iter().map(Into::into).collect()));
    }
    Ok(
        known(beep.call(ListProjectPullRequests { project_id }).await)?
            .map(|rows| rows.into_iter().map(Into::into).collect()),
    )
}

/// The branch pull request `number` is made from, for Continue.
#[tauri::command]
pub async fn center_pull_request_head(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project_id: Uuid,
    number: u64,
) -> Result<PullRequestHead> {
    if let Some((desk, name)) = mesh_project(&fleet, project_id).await? {
        return Ok(desk.pr_head(&name, number).await?);
    }
    beep.call(GetPullRequestHead { project_id, number })
        .await
        .map_err(outdated)
}

/// Asks the project agent to draft a reply; gives the project chat.
#[tauri::command]
pub async fn center_reply_thread(
    beep: Service<'_>,
    project_id: Uuid,
    repo: String,
    number: u64,
) -> Result<Uuid> {
    beep.call(ReplyToThread {
        project_id,
        repo,
        number,
    })
    .await
    .map_err(outdated)
}

#[tauri::command]
pub async fn center_open_thread_chat(
    beep: Service<'_>,
    project_id: Uuid,
    repo: String,
    number: u64,
) -> Result<ThreadChatRow> {
    let chat = beep
        .call(OpenThreadChat {
            project_id,
            repo,
            number,
        })
        .await
        .map_err(outdated)?;
    Ok(chat.into())
}

/// The project's thread chats; `None` with no server or one too old to keep
/// them.
#[tauri::command]
pub async fn center_thread_chats(
    beep: Service<'_>,
    project_id: Uuid,
) -> Result<Option<Vec<ThreadChatRow>>> {
    if !beep.has_server() {
        return Ok(None);
    }
    Ok(known(beep.call(ListThreadChats { project_id }).await)?
        .map(|rows| rows.into_iter().map(Into::into).collect()))
}

/// Thread `number` of `repo` read from its host now; `None` from a server
/// too old to read one.
#[tauri::command]
pub async fn center_thread_view(
    beep: Service<'_>,
    project_id: Uuid,
    repo: String,
    number: u64,
) -> Result<Option<ThreadCardRow>> {
    let read = beep.call(GetProjectThread {
        project_id,
        repo,
        number,
    });
    Ok(known(read.await)?.map(Into::into))
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
}

impl From<ThreadDetail> for ThreadDetailRow {
    fn from(t: ThreadDetail) -> Self {
        Self {
            kind: kind_name(t.kind),
            number: t.number,
            title: t.title,
            url: t.url,
            author: t.author,
            state: t.state.into(),
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
            review: t.review.into(),
            checks: t.checks.map(Into::into),
        }
    }
}

/// Pull request or issue `number` of the project read in full from its
/// host now, through a machine of the mesh that has the project, else the
/// server; `None` from a server too old to read one.
#[tauri::command]
pub async fn center_thread_detail(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project_id: Uuid,
    number: u64,
) -> Result<Option<ThreadDetailRow>> {
    if let Some((desk, name)) = mesh_project(&fleet, project_id).await? {
        return Ok(Some(desk.thread_detail(&name, number).await?.into()));
    }
    Ok(known(beep.call(GetThreadDetail { project_id, number }).await)?.map(Into::into))
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
    let Some((desk, name)) = mesh_project(&fleet, project_id).await? else {
        return Err(DesktopError::NoServer);
    };
    let kind = match kind {
        ThreadKind::Pr => GitItemKind::PullRequest {},
        ThreadKind::Issue => GitItemKind::Issue {},
    };
    Ok(desk.comment_thread(&name, kind, number, &body).await?)
}
