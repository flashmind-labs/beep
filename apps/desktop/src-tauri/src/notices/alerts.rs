//! The assistant's alerts: what it told the user on its own, such as a mail
//! digest or an arrival. They are notifications, not chats, until the user
//! replies in one. The lists leave them out, and the bell in the command
//! center's header lists them.

use super::{notify, route};
use crate::{
    beep::{Beep, Service, epoch_ms, state::LinkState},
    error::Result,
    projects::known,
    voice::Voice,
};
use beep_proto::{Alert, DismissAlerts, ListAlerts, Notice, ReadAlerts};
use serde::Serialize;
use std::{collections::HashSet, sync::Arc};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

/// One alert as the bell shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlertView {
    session_id: Uuid,
    title: String,
    source: String,
    text: String,
    read: bool,
    /// Milliseconds since the Unix epoch.
    at: u64,
}

impl From<Alert> for AlertView {
    fn from(alert: Alert) -> Self {
        Self {
            session_id: alert.session_id,
            title: alert.title,
            source: alert.source,
            text: alert.text,
            read: alert.read,
            at: epoch_ms(alert.at),
        }
    }
}

/// The alerts, and the lists leave their chats out. `None` when there is
/// nothing to ask: no server, an app not linked to one, or a server from
/// before alerts, whose assistant notices are chats as they always were.
pub(crate) async fn fetch(app: &AppHandle, beep: &Beep) -> Option<Vec<AlertView>> {
    let listed = if beep.link_state() == LinkState::Linked {
        match known(beep.call(ListAlerts {}).await) {
            Ok(listed) => listed,
            Err(e) => {
                log::info!("reading the alerts: {e}");
                None
            }
        }
    } else {
        None
    };
    let hidden: HashSet<Uuid> = listed
        .iter()
        .flatten()
        .map(|alert| alert.session_id)
        .collect();
    if beep.chats().hide_alerts(hidden) {
        // The chat lists redraw without them, or with the one just answered.
        let _ = app.emit("sessions-changed", "");
    }
    listed.map(|alerts| alerts.into_iter().map(AlertView::from).collect())
}

/// Reads the alerts again and tells every window.
pub(crate) async fn refresh(app: &AppHandle) {
    let beep = app.state::<Arc<Beep>>().inner().clone();
    let alerts = fetch(app, &beep).await;
    if let Err(e) = app.emit("alerts", alerts) {
        log::warn!("telling the windows about the alerts: {e}");
    }
}

/// A chat notice that may be an alert. An alert shows as a macOS
/// notification, which opens it in the bell's pane, and as a row there;
/// nothing else opens by itself. Any other chat notice goes on as usual.
pub(super) async fn arrived(app: AppHandle, beep: Arc<Beep>, voice: Arc<Voice>, notice: Notice) {
    let alerts = fetch(&app, &beep).await;
    let is_alert = alerts.as_ref().is_some_and(|alerts| {
        alerts
            .iter()
            .any(|alert| alert.session_id.to_string() == notice.session_id)
    });
    if let Err(e) = app.emit("alerts", alerts) {
        log::warn!("telling the windows about the alerts: {e}");
    }
    if is_alert {
        notify::alert(&app, &notice);
    } else {
        route::chat_notice(&app, &beep, &voice, notice);
    }
}

/// The alerts, newest first, or `None` when there are none to have.
#[tauri::command]
pub async fn alerts_list(app: AppHandle, beep: Service<'_>) -> Result<Option<Vec<AlertView>>> {
    Ok(fetch(&app, &beep).await)
}

/// Marks alerts read, by `AlertView::session_id`.
#[tauri::command]
pub async fn alerts_read(app: AppHandle, beep: Service<'_>, ids: Vec<Uuid>) -> Result<()> {
    beep.call(ReadAlerts { ids }).await?;
    refresh(&app).await;
    Ok(())
}

/// Deletes alerts, by `AlertView::session_id`.
#[tauri::command]
pub async fn alerts_dismiss(app: AppHandle, beep: Service<'_>, ids: Vec<Uuid>) -> Result<()> {
    beep.call(DismissAlerts { ids }).await?;
    refresh(&app).await;
    Ok(())
}
