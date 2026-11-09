//! Pending approval cards from the caller's project chats.

use super::Reply;
use crate::{
    account::user::User,
    api::router::Cx,
    protocol::{ChatApproval, ListPendingApprovals},
};
use std::collections::HashMap;

pub(super) async fn list_pending_approvals(
    cx: Cx,
    _: ListPendingApprovals,
) -> Reply<Vec<ChatApproval>> {
    pending_approvals(&cx.user).await
}

/// The cards the project chats wait on.
pub(super) async fn pending_approvals(user: &User) -> Reply<Vec<ChatApproval>> {
    let pending = super::projects::project_cards(user).await?;

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
