//! Commands for one-to-one chats: sending and rerunning prompts, following
//! runs, and listing, opening, renaming, pinning, sharing and deleting chats.

use crate::{
    beep::{BarEvent, DataImage, FileUpload, Pick, Sent, Service, Turn, chat_id},
    center::{self, Chat, Shown},
    error::{DesktopError, Result},
    projects::unknown_op,
};
use beep_proto::{
    CancelRun, Chart, ChartRange, DeleteSession, GetRouteCard, GetSessionAgents, GetSessionFiles,
    GetSessionSources, ListActiveRuns, PinSession, RenameSession, RouteCard, SentFiles, Session,
    SessionMatch, ShareSession, SharedLink, StockChart, TurnAgent, TurnSources, UnshareSession,
};
use tauri::{State, WebviewWindow, ipc::Channel};
use uuid::Uuid;

#[tauri::command]
pub async fn send_prompt(
    window: WebviewWindow,
    beep: Service<'_>,
    session: Option<String>,
    message: String,
    images: Vec<DataImage>,
    files: Vec<FileUpload>,
    on_event: Channel<BarEvent>,
) -> Result<String> {
    let sent = Sent {
        message,
        images,
        files,
        research: false,
        voice: None,
        spoken: false,
    };
    let session_id = chat_id(session.as_deref());
    // Made current before the run, so its own notices stay quiet.
    center::remember_for(&window, &beep, Chat::Session(Some(session_id.to_string())));
    beep.post(session_id, sent, Pick::default(), on_event)
        .await?;
    Ok(session_id.to_string())
}

/// Sends the chat's `index`th prompt again on another model or thinking,
/// dropping it and the turns after it first: an admin's Rerun. An empty
/// `pick` is Retry, which anyone may use: the account's own pick.
#[tauri::command]
pub async fn rerun_prompt(
    window: WebviewWindow,
    beep: Service<'_>,
    session: String,
    index: usize,
    pick: Pick,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    center::remember_for(&window, &beep, Chat::Session(Some(session.clone())));
    beep.rerun(session, index, pick, on_event).await
}

#[tauri::command]
pub async fn active_run(beep: Service<'_>, session: String) -> Result<Option<String>> {
    beep.active_run(&session).await
}

/// The chats with a run going on, started here or on another device, by
/// session id. Empty from a server too old to say.
#[tauri::command]
pub async fn active_runs(beep: Service<'_>) -> Result<Vec<String>> {
    match beep.call(ListActiveRuns {}).await {
        Ok(runs) => Ok(runs.into_iter().map(|run| run.session_id).collect()),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn follow_run(
    beep: Service<'_>,
    run_id: String,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    beep.follow_run(run_id, on_event).await
}

#[tauri::command]
pub async fn cancel_run(beep: Service<'_>, run_id: String) -> Result<()> {
    beep.call(CancelRun { id: run_id }).await
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
pub async fn open_session(
    window: WebviewWindow,
    beep: Service<'_>,
    id: String,
) -> Result<Vec<Turn>> {
    // Made current before loading, so leaving the chat meanwhile is not undone.
    center::remember_for(&window, &beep, Chat::Session(Some(id.clone())));
    beep.history(id).await
}

/// The files other than pictures sent in a session, by the `at` of the
/// user turn they came with.
#[tauri::command]
pub async fn session_files(beep: Service<'_>, id: String) -> Result<Vec<SentFiles>> {
    beep.call(GetSessionFiles { session_id: id }).await
}

/// The web pages each turn of a session used, by the `at` of its user
/// turn, for the source chips under its reply.
#[tauri::command]
pub async fn session_sources(beep: Service<'_>, id: String) -> Result<Vec<TurnSources>> {
    let session_id =
        Uuid::parse_str(&id).map_err(|_| DesktopError::BadRequest("not a chat".into()))?;
    beep.call(GetSessionSources { session_id }).await
}

/// The agent each turn of session `id` ran as, by the `at` of its user turn.
#[tauri::command]
pub async fn session_agents(beep: Service<'_>, id: String) -> Result<Vec<TurnAgent>> {
    beep.call(GetSessionAgents { session_id: id }).await
}

#[tauri::command]
pub fn new_chat(window: WebviewWindow, beep: Service<'_>) {
    center::remember_for(&window, &beep, Chat::Session(None));
}

#[tauri::command]
pub async fn rename_session(beep: Service<'_>, id: String, title: String) -> Result<()> {
    beep.call(RenameSession { id, title }).await
}

#[tauri::command]
pub async fn pin_session(beep: Service<'_>, id: String, pinned: bool) -> Result<()> {
    beep.call(PinSession { id, pinned }).await
}

#[tauri::command]
pub async fn delete_session(beep: Service<'_>, shown: State<'_, Shown>, id: String) -> Result<()> {
    beep.call(DeleteSession { id: id.clone() }).await?;
    if let Ok(chat) = Uuid::parse_str(&id) {
        beep.chats().forget(chat);
    }
    if beep.session().as_deref() == Some(id.as_str()) {
        beep.set_session(None);
    }
    shown.forget_session(&id);
    Ok(())
}

#[tauri::command]
pub async fn share_session(beep: Service<'_>, id: String) -> Result<SharedLink> {
    beep.call(ShareSession { id }).await
}

#[tauri::command]
pub async fn unshare_session(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(UnshareSession { id }).await
}

/// A stock chart over another range, for a chart's range buttons.
#[tauri::command]
pub async fn stock_chart(beep: Service<'_>, symbol: String, range: ChartRange) -> Result<Chart> {
    beep.call(StockChart { symbol, range }).await
}

/// The transit card a `get_route` call planned.
#[tauri::command]
pub async fn route_card(beep: Service<'_>, call_id: String) -> Result<RouteCard> {
    beep.call(GetRouteCard { call_id }).await
}
