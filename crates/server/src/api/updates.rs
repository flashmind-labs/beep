//! What the event triage agent kept for the user.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{ApiError, AppUpdate, DismissUpdates, ListUpdates, OpenUpdateChat, Session},
    routines::events::{self, EventApp, update_chat},
};
use std::time::SystemTime;

pub(super) async fn list_updates(cx: Cx, _: ListUpdates) -> Reply<Vec<AppUpdate>> {
    let rows = events::waiting_updates(&cx.user).await?;
    Ok(rows
        .into_iter()
        .map(|row| AppUpdate {
            id: row.id,
            summary: row.summary,
            app: row.app.map(EventApp::slug).unwrap_or_default().to_owned(),
            app_name: row.app.map(EventApp::name).unwrap_or_default().to_owned(),
            at: SystemTime::from(row.decided_at),
        })
        .collect())
}

pub(super) async fn dismiss_updates(cx: Cx, DismissUpdates { ids }: DismissUpdates) -> Reply<()> {
    if !ids.is_empty() {
        cx.user.mark_delivered(&ids, None).await?;
    }
    Ok(())
}

/// The chat about an update, opening with Beep telling the user it.
pub(super) async fn open_update_chat(
    cx: Cx,
    OpenUpdateChat { id }: OpenUpdateChat,
) -> Reply<Session> {
    let chat = update_chat::open(&cx.user, id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    cx.user.session_of(chat).await?.ok_or(ApiError::Internal {})
}
