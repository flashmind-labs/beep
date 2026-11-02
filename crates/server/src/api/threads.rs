//! The user's pull requests and issues in a project, for the apps: which
//! wait on a reply, asking the agent for one, and the chats about one.

use super::{
    Reply,
    projects::{issue_refusal, project_of},
};
use crate::{
    app::App,
    projects::{
        fleet::NodeFleet,
        model::ProjectId,
        threads::{self, ListedThread},
    },
    protocol::*,
    router::Cx,
    store::ThreadChatRow,
    user::User,
};
use std::sync::Arc;
use uuid::Uuid;

pub(super) async fn list_project_threads(
    cx: Cx,
    req: ListProjectThreads,
) -> Reply<Vec<ProjectThread>> {
    project_threads(&cx.app, &cx.user, ProjectId(req.project_id)).await
}

pub(super) async fn reply_to_thread(cx: Cx, req: ReplyToThread) -> Reply<Uuid> {
    reply(
        &cx.app,
        &cx.user,
        ProjectId(req.project_id),
        &req.repo,
        req.number,
    )
    .await
}

pub(super) async fn open_thread_chat(cx: Cx, req: OpenThreadChat) -> Reply<ThreadChat> {
    open_chat(
        &cx.app,
        &cx.user,
        ProjectId(req.project_id),
        &req.repo,
        req.number,
    )
    .await
}

pub(super) async fn list_thread_chats(cx: Cx, req: ListThreadChats) -> Reply<Vec<ThreadChat>> {
    thread_chats(&cx.user, ProjectId(req.project_id)).await
}

fn fleet(app: &Arc<App>, user: &Arc<User>) -> NodeFleet {
    NodeFleet {
        app: app.clone(),
        user: user.clone(),
    }
}

fn thread_view(thread: ListedThread) -> ProjectThread {
    ProjectThread {
        kind: thread.kind.into(),
        repo: thread.repo,
        number: thread.number,
        title: thread.title,
        url: thread.url,
        last_other: thread.last_other,
        needs_reply: thread.needs_reply,
        updated_at: thread.updated_at.into(),
    }
}

fn chat_view(chat: ThreadChatRow) -> ThreadChat {
    ThreadChat {
        chat_id: chat.chat,
        kind: chat.kind.into(),
        repo: chat.repo,
        number: chat.number,
        title: chat.title,
    }
}

async fn project_threads(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
) -> Reply<Vec<ProjectThread>> {
    let project = project_of(user, id).await?;
    let listed = threads::list(&fleet(app, user), user, &project)
        .await
        .map_err(issue_refusal)?;
    Ok(listed.into_iter().map(thread_view).collect())
}

async fn reply(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
    repo: &str,
    number: u64,
) -> Reply<Uuid> {
    let project = project_of(user, id).await?;
    threads::ask_for_reply(&fleet(app, user), user, &project, repo, number)
        .await
        .map_err(issue_refusal)
}

async fn open_chat(
    app: &Arc<App>,
    user: &Arc<User>,
    id: ProjectId,
    repo: &str,
    number: u64,
) -> Reply<ThreadChat> {
    let project = project_of(user, id).await?;
    let chat = threads::open_chat(&fleet(app, user), user, &project, repo, number)
        .await
        .map_err(issue_refusal)?;
    Ok(chat_view(chat))
}

async fn thread_chats(user: &User, id: ProjectId) -> Reply<Vec<ThreadChat>> {
    let project = project_of(user, id).await?;
    Ok(user
        .thread_chats(project.id)
        .await?
        .into_iter()
        .map(chat_view)
        .collect())
}
