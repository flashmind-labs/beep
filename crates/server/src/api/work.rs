//! A chat's background work; now always empty, kept for apps that still ask.

use super::Reply;
use crate::{
    account::user::User,
    api::router::Cx,
    app::App,
    protocol::{
        ApiError, CancelWork, ChatApproval, ListPendingApprovals, ListQuestions, ListWork,
        PendingApproval, WorkItem,
    },
};
use std::collections::HashMap;

pub(super) async fn list_work(_: Cx, _: ListWork) -> Reply<Vec<WorkItem>> {
    Ok(Vec::new())
}

pub(super) async fn cancel_work(_: Cx, _: CancelWork) -> Reply<bool> {
    Err(ApiError::NotFound {})
}

pub(super) async fn list_questions(
    cx: Cx,
    ListQuestions { session_id }: ListQuestions,
) -> Reply<Vec<PendingApproval>> {
    Ok(cx.app.subagents.approvals.pending(cx.user.id(), session_id))
}

pub(super) async fn list_pending_approvals(
    cx: Cx,
    _: ListPendingApprovals,
) -> Reply<Vec<ChatApproval>> {
    pending_approvals(&cx.app, &cx.user).await
}

/// What background work waits on, then the cards the project chats wait on.
pub(super) async fn pending_approvals(app: &App, user: &User) -> Reply<Vec<ChatApproval>> {
    let mut pending = app.subagents.approvals.pending_all(user.id());
    pending.extend(super::projects::project_cards(user).await?);
    if pending.is_empty() {
        return Ok(Vec::new());
    }
    let titles: HashMap<String, String> = user
        .sessions()
        .await?
        .into_iter()
        .map(|session| (session.id, session.title))
        .collect();
    Ok(pending
        .into_iter()
        .map(|(chat, pending)| ChatApproval {
            session_id: chat,
            session_title: titles.get(&chat.to_string()).cloned().unwrap_or_default(),
            pending,
        })
        .collect())
}
