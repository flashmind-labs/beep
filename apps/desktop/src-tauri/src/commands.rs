//! What the bar's page can ask the app to do.

use crate::{
    bar,
    beep::{Answer, BarEvent, Beep, LinkView, Turn},
    error::{DesktopError, Result},
    state::LinkState,
};
use beep_proto::{ApprovalDecision, Session, SessionMatch};
use serde::{Deserialize, Serialize};
use std::{process::Command, sync::Arc};
use tauri::{AppHandle, State, ipc::Channel};

type Service<'a> = State<'a, Arc<Beep>>;

#[derive(Serialize)]
pub struct Status {
    link: LinkState,
    session: Option<String>,
}

#[tauri::command]
pub fn status(beep: Service<'_>) -> Status {
    Status {
        link: beep.link_state(),
        session: beep.session(),
    }
}

#[tauri::command]
pub fn link_offer(beep: Service<'_>) -> Result<LinkView> {
    beep.link_offer(&crate::computer_name())
}

#[tauri::command]
pub async fn send_prompt(
    beep: Service<'_>,
    session: Option<String>,
    message: String,
    on_event: Channel<BarEvent>,
) -> Result<String> {
    beep.send(session, message, on_event).await
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Always,
    Deny,
}

impl From<Decision> for ApprovalDecision {
    fn from(d: Decision) -> Self {
        match d {
            Decision::Allow => Self::Allow {},
            Decision::Always => Self::Always {},
            Decision::Deny => Self::Deny {},
        }
    }
}

#[tauri::command]
pub async fn answer_approval(
    beep: Service<'_>,
    session: String,
    query_id: String,
    decision: Decision,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    beep.answer(Answer {
        session,
        query_id,
        decision: decision.into(),
        channel: on_event,
    })
    .await
}

#[tauri::command]
pub async fn cancel_run(beep: Service<'_>, run_id: String) -> Result<()> {
    beep.cancel(run_id).await
}

#[tauri::command]
pub async fn list_sessions(beep: Service<'_>) -> Result<Vec<Session>> {
    beep.sessions().await
}

#[tauri::command]
pub async fn search_sessions(beep: Service<'_>, query: String) -> Result<Vec<SessionMatch>> {
    beep.search(query).await
}

#[tauri::command]
pub async fn open_session(beep: Service<'_>, id: String) -> Result<Vec<Turn>> {
    let turns = beep.history(id.clone()).await?;
    beep.set_session(Some(id));
    Ok(turns)
}

#[tauri::command]
pub fn new_chat(beep: Service<'_>) {
    beep.set_session(None);
}

#[tauri::command]
pub async fn unlink(beep: Service<'_>) -> Result<()> {
    beep.unlink().await
}

/// Forgets this computer's key and starts again with a new one, the only
/// way back after the phone removed it.
#[tauri::command]
pub fn start_over(app: AppHandle) -> Result<()> {
    crate::forget_identity(&app)?;
    app.restart()
}

#[tauri::command]
pub fn resize_bar(app: AppHandle, height: f64) -> Result<()> {
    Ok(bar::resize(&app, height)?)
}

#[tauri::command]
pub fn hide_bar(app: AppHandle) {
    bar::hide(&app);
}

/// The app has no menu bar of its own, so the bar handles ⌘Q.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/// Opens a link from a reply in the browser, never in the bar.
#[tauri::command]
pub fn open_url(url: String) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(DesktopError::BadRequest("only web links open".into()));
    }
    Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}
