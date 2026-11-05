//! What the assistant told the user on its own, apart from their chats.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{Alert, ApiError, DismissAlerts, ListAlerts, ReadAlerts},
};
use std::time::SystemTime;
use uuid::Uuid;

/// Alerts one request may name.
const MAX_IDS: usize = 200;

fn bounded(ids: &[Uuid]) -> Reply<()> {
    if ids.len() > MAX_IDS {
        return Err(ApiError::bad_request("at most 200 alerts at once"));
    }
    Ok(())
}

pub(super) async fn list_alerts(cx: Cx, _: ListAlerts) -> Reply<Vec<Alert>> {
    let rows = cx.user.alerts().await?;
    Ok(rows
        .into_iter()
        .map(|row| Alert {
            session_id: row.chat,
            title: row.title,
            source: row.source,
            text: row.text,
            read: row.read,
            at: SystemTime::from(row.at),
        })
        .collect())
}

pub(super) async fn read_alerts(cx: Cx, ReadAlerts { ids }: ReadAlerts) -> Reply<()> {
    bounded(&ids)?;
    cx.user.read_alerts(&ids).await?;
    Ok(())
}

pub(super) async fn dismiss_alerts(cx: Cx, DismissAlerts { ids }: DismissAlerts) -> Reply<()> {
    bounded(&ids)?;
    cx.user.dismiss_alerts(&ids).await?;
    Ok(())
}
