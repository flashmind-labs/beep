//! What the command center and the bar ask of a research: starting and
//! opening one, listing them, its chat, stopping it and exporting its paper.

use crate::{
    beep::{BarEvent, Pick, Sent, Service, Turn},
    center::{self, CenterTarget},
    chat::files::{file_name, free_name},
    error::{DesktopError, Result},
};
use beep_proto::{GetResearchPdf, ListResearches, ResearchSummary, StopResearch};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};
use tauri::{AppHandle, Manager, State, ipc::Channel};
use uuid::Uuid;

/// Topics typed in the bar, waiting for the research's page to send them.
#[derive(Default)]
pub struct Pending(Mutex<HashMap<Uuid, String>>);

impl Pending {
    fn put(&self, session: Uuid, topic: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session, topic);
    }

    fn take(&self, session: Uuid) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&session)
    }
}

/// Starts a research on `topic` in a new chat and opens it in the command
/// center, whose page sends the topic. Returns the chat.
#[tauri::command]
pub fn research_begin(
    app: AppHandle,
    beep: Service<'_>,
    pending: State<'_, Pending>,
    window: State<'_, center::Pending>,
    topic: String,
) -> Result<Uuid> {
    let topic = topic.trim();
    if topic.is_empty() {
        return Err(DesktopError::BadRequest("say what to research".into()));
    }
    let session = Uuid::new_v4();
    beep.add_research(session);
    pending.put(session, topic.to_owned());
    center::open_on(&app, &window, CenterTarget::OpenResearch { session })?;
    Ok(session)
}

/// Opens a research started earlier in the command center.
#[tauri::command]
pub fn open_research(
    app: AppHandle,
    beep: Service<'_>,
    window: State<'_, center::Pending>,
    session: Uuid,
) -> Result<()> {
    beep.add_research(session);
    center::open_on(&app, &window, CenterTarget::OpenResearch { session })
}

/// The chats started here as researches.
#[tauri::command]
pub fn research_sessions(beep: Service<'_>) -> Vec<Uuid> {
    beep.researches()
}

/// Every research of the account, newest first, wherever it started. Each
/// is remembered here, so its chat opens as a research.
#[tauri::command]
pub async fn list_researches(beep: Service<'_>) -> Result<Vec<ResearchItem>> {
    let listed = beep.call(ListResearches {}).await?;
    let sessions: Vec<Uuid> = listed.iter().map(|r| r.session_id).collect();
    beep.add_researches(&sessions);
    Ok(listed.into_iter().map(ResearchItem::from).collect())
}

#[derive(Serialize)]
pub struct ResearchItem {
    session: Uuid,
    topic: String,
    status: &'static str,
    round: u32,
    sources: u32,
    started_at: i64,
    finished_at: Option<i64>,
}

impl From<ResearchSummary> for ResearchItem {
    fn from(research: ResearchSummary) -> Self {
        Self {
            session: research.session_id,
            topic: research.topic,
            status: research.status.as_str(),
            round: research.round,
            sources: research.sources,
            started_at: research.started_at,
            finished_at: research.finished_at,
        }
    }
}

/// The topic the research's page should send first, once.
#[tauri::command]
pub fn research_pending(pending: State<'_, Pending>, session: Uuid) -> Option<String> {
    pending.take(session)
}

/// The lead's chat.
#[tauri::command]
pub async fn research_history(beep: Service<'_>, session: Uuid) -> Result<Vec<Turn>> {
    beep.history(session.to_string()).await
}

/// Sends a message to the lead. `start` makes the chat a research.
#[tauri::command]
pub async fn research_send(
    beep: Service<'_>,
    session: Uuid,
    message: String,
    start: bool,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    let sent = Sent {
        message,
        images: Vec::new(),
        files: Vec::new(),
        research: start,
        voice: None,
        spoken: false,
    };
    beep.post(session, sent, Pick::default(), on_event).await?;
    Ok(())
}

#[tauri::command]
pub async fn stop_research(beep: Service<'_>, session: Uuid) -> Result<()> {
    beep.call(StopResearch {
        session_id: session,
    })
    .await
}

/// Saves the paper, typeset as a PDF by the server, in Downloads.
#[tauri::command]
pub async fn export_research(app: AppHandle, beep: Service<'_>, session: Uuid) -> Result<PathBuf> {
    let pdf = beep
        .call(GetResearchPdf {
            session_id: session,
        })
        .await?;
    let to = free_name(&app.path().download_dir()?, &file_name(&pdf.name));
    fs::write(&to, pdf.data).map_err(anyhow::Error::from)?;
    Ok(to)
}
