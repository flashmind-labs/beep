//! Research windows: one per research chat, showing the lead's chat, the
//! team's workspace and its agents. The bar starts researches and opens
//! them; the window polls the server for what the team did.

use crate::{
    bar,
    beep::{BarEvent, Beep, Pick, Sent, Turn},
    commands::{file_name, free_name},
    error::{DesktopError, Result},
    markdown,
};
use beep_proto::{
    GetResearch, GetResearchPdf, ListResearches, Research, ResearchEntryBody, ResearchSummary,
    StopResearch,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};
use tauri::{
    ActivationPolicy, AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, State,
    TitleBarStyle, WebviewUrl, WebviewWindowBuilder, ipc::Channel,
};
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

/// Smallest size a new window opens at, before it is fitted to the screen.
const WIDTH: f64 = 1080.0;
const HEIGHT: f64 = 700.0;
/// Share of the screen a new window takes.
const SHARE: f64 = 0.82;
/// Each window opened while others are open sits this much lower and right.
const CASCADE: f64 = 28.0;

/// Topics typed in the bar, waiting for their window to send them.
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

const PREFIX: &str = "research-";

fn label(session: Uuid) -> String {
    format!("{PREFIX}{}", session.simple())
}

/// Research windows open, leaving out `closing`, which may still be listed.
fn open_windows<R: Runtime>(app: &AppHandle<R>, closing: Option<&str>) -> usize {
    app.webview_windows()
        .keys()
        .filter(|label| label.starts_with(PREFIX) && Some(label.as_str()) != closing)
        .count()
}

/// Research windows are ordinary windows: while one is open Beep sits in
/// the Dock and Cmd-Tab, and goes back to the menu bar once the last
/// closes.
pub fn closed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if label.starts_with(PREFIX)
        && open_windows(app, Some(label)) == 0
        && let Err(e) = app.set_activation_policy(ActivationPolicy::Accessory)
    {
        log::warn!("leaving the Dock: {e}");
    }
}

/// Where a new window goes: most of the screen under the mouse, moved down
/// and right for each window already open so none hides another.
fn frame<R: Runtime>(
    app: &AppHandle<R>,
    open: usize,
) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let cursor = app.cursor_position().ok()?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (left, top) = (
        f64::from(area.position.x) / scale,
        f64::from(area.position.y) / scale,
    );
    let (full_w, full_h) = (
        f64::from(area.size.width) / scale,
        f64::from(area.size.height) / scale,
    );
    let width = (full_w * SHARE).max(WIDTH.min(full_w));
    let height = (full_h * SHARE).max(HEIGHT.min(full_h));
    let step = CASCADE * (open % 8) as f64;
    let x = left + ((full_w - width) / 2.0 + step).min(full_w - width).max(0.0);
    let y = top
        + ((full_h - height) / 2.0 + step)
            .min(full_h - height)
            .max(0.0);
    Some((LogicalPosition::new(x, y), LogicalSize::new(width, height)))
}

/// Shows the research window for `session`, opening it if needed.
fn open_window<R: Runtime>(app: &AppHandle<R>, session: Uuid) -> Result<()> {
    if let Some(window) = app.get_webview_window(&label(session)) {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    if let Err(e) = app.set_activation_policy(ActivationPolicy::Regular) {
        log::warn!("joining the Dock: {e}");
    }
    let url = WebviewUrl::App(format!("research.html?session={session}").into());
    let mut builder = WebviewWindowBuilder::new(app, label(session), url)
        .title("Research")
        .min_inner_size(820.0, 520.0)
        .resizable(true)
        .maximizable(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true);
    builder = match frame(app, open_windows(app, None)) {
        Some((position, size)) => builder
            .position(position.x, position.y)
            .inner_size(size.width, size.height),
        None => builder.inner_size(WIDTH, HEIGHT).center(),
    };
    let window = builder.build()?;
    window.set_focus()?;
    Ok(())
}

/// Starts a research on `topic` in a new chat and opens its window, which
/// sends the topic. Returns the chat.
#[tauri::command]
pub fn research_begin(
    app: AppHandle,
    beep: Service<'_>,
    pending: State<'_, Pending>,
    topic: String,
) -> Result<Uuid> {
    let topic = topic.trim();
    if topic.is_empty() {
        return Err(DesktopError::BadRequest("say what to research".into()));
    }
    let session = Uuid::new_v4();
    beep.add_research(session);
    pending.put(session, topic.to_owned());
    open_window(&app, session)?;
    bar::hide(&app);
    Ok(session)
}

/// Opens the window of a research started earlier.
#[tauri::command]
pub fn open_research(app: AppHandle, beep: Service<'_>, session: Uuid) -> Result<()> {
    beep.add_research(session);
    open_window(&app, session)?;
    bar::hide(&app);
    Ok(())
}

/// The chats started here as researches.
#[tauri::command]
pub fn research_sessions(beep: Service<'_>) -> Vec<Uuid> {
    beep.researches()
}

/// Every research of the account, newest first, wherever it started. Each
/// is remembered here, so its chat opens in a research window.
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

/// The topic the window should send first, once.
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
    };
    beep.post(session, sent, Pick::default(), on_event).await
}

/// A research as the window draws it.
#[derive(Serialize)]
pub struct ResearchView {
    topic: String,
    status: &'static str,
    agents: Vec<AgentView>,
    entries: Vec<EntryView>,
    sources: Vec<SourceView>,
    /// The paper as Markdown, for copying and export.
    draft: String,
    /// The paper as sanitized HTML.
    draft_html: String,
    started_at: i64,
    finished_at: Option<i64>,
    /// The newest round, from 1.
    round: u32,
}

#[derive(Serialize)]
struct AgentView {
    id: String,
    name: String,
    role: String,
    model: String,
    /// A scout's round; 0 for agents in every round.
    round: u32,
}

#[derive(Serialize)]
struct SourceView {
    number: u32,
    url: String,
    title: String,
    quote: String,
    agent: String,
}

#[derive(Serialize)]
struct EntryView {
    seq: i64,
    agent: String,
    at: i64,
    #[serde(flatten)]
    body: EntryBody,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum EntryBody {
    Said { to: Option<String>, content: String },
    Step { label: String, success: bool },
    Note { content: String, sources: Vec<u32> },
}

impl From<ResearchEntryBody> for EntryBody {
    fn from(body: ResearchEntryBody) -> Self {
        match body {
            ResearchEntryBody::Said { to, content } => Self::Said { to, content },
            ResearchEntryBody::Step { label, success } => Self::Step { label, success },
            ResearchEntryBody::Note { content, sources } => Self::Note { content, sources },
        }
    }
}

impl From<Research> for ResearchView {
    fn from(research: Research) -> Self {
        Self {
            topic: research.topic,
            status: research.status.as_str(),
            agents: research
                .agents
                .into_iter()
                .map(|agent| AgentView {
                    id: agent.id,
                    name: agent.name,
                    role: agent.role,
                    model: agent.model,
                    round: agent.round,
                })
                .collect(),
            entries: research
                .entries
                .into_iter()
                .map(|entry| EntryView {
                    seq: entry.seq,
                    agent: entry.agent,
                    at: entry.timestamp,
                    body: entry.body.into(),
                })
                .collect(),
            sources: research
                .sources
                .into_iter()
                .map(|source| SourceView {
                    number: source.number,
                    url: source.url,
                    title: source.title,
                    quote: source.quote,
                    agent: source.agent,
                })
                .collect(),
            draft_html: markdown::to_html(&research.draft),
            draft: research.draft,
            started_at: research.started_at,
            finished_at: research.finished_at,
            round: research.round,
        }
    }
}

/// What the team did, from entry `after` on. `None` until the server has
/// made the chat a research.
#[tauri::command]
pub async fn get_research(
    beep: Service<'_>,
    session: Uuid,
    after: i64,
) -> Result<Option<ResearchView>> {
    let research = beep
        .call(GetResearch {
            session_id: session,
            after,
        })
        .await?;
    Ok(research.map(ResearchView::from))
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
