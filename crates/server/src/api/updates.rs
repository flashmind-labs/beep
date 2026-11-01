//! What the event triage agent kept for the user, shown by the apps on its
//! own rather than inside a reply.

use super::Reply;
use crate::{
    events::{self, EventApp},
    protocol::{AppUpdate, DismissUpdates, ListUpdates},
    router::Cx,
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
