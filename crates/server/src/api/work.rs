//! A chat's background work and the questions it waits on. The main agent no
//! longer starts background work, so `ListWork` is always empty and
//! `CancelWork` finds nothing; both stay for the apps that still ask.

use super::Reply;
use crate::{
    protocol::{ApiError, CancelWork, ListQuestions, ListWork, PendingApproval, WorkItem},
    router::Cx,
};

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
