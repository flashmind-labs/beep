//! A chat's background work and the questions it waits on. The main agent no
//! longer starts background work, so `ListWork` is always empty and
//! `CancelWork` finds nothing; both stay for the apps that still ask.

use super::Reply;
use crate::{
    protocol::{
        ApiError, CancelWork, ChatApproval, ListPendingApprovals, ListQuestions, ListWork,
        PendingApproval, WorkItem,
    },
    router::Cx,
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
    let pending = cx.app.subagents.approvals.pending_all(cx.user.id());
    if pending.is_empty() {
        return Ok(Vec::new());
    }
    let titles: HashMap<String, String> = cx
        .user
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
