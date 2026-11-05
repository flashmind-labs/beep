//! Commands for one-to-one chats: sending and rerunning prompts, following
//! runs, and listing, opening, renaming, pinning, sharing and deleting chats.

use crate::{
    assistant::{
        Runs,
        local::{self, Prompt},
    },
    beep::{BarEvent, DataImage, FileUpload, Pick, Sent, Service, Turn, chat_id},
    center::{self, Chat, Shown},
    error::{DesktopError, Result},
    fleet::projects::FleetState,
    projects::unknown_op,
};
use beep_proto::{
    CancelRun, Chart, ChartRange, DeleteSession, GetRouteCard, GetSessionAgents, GetSessionFiles,
    GetSessionSources, ListActiveRuns, PinSession, RenameSession, RouteCard, SentFiles, Session,
    SessionMatch, ShareSession, SharedLink, StockChart, TurnAgent, TurnSources, UnshareSession,
};
use tauri::{AppHandle, State, WebviewWindow, ipc::Channel};
use uuid::Uuid;

// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn send_prompt(
    app: AppHandle,
    window: WebviewWindow,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    runs: State<'_, Runs>,
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
    if local::on(&beep, &fleet).await {
        if !sent.images.is_empty() || !sent.files.is_empty() {
            return Err(DesktopError::BadRequest(
                "Pictures and files are not available in this chat yet.".into(),
            ));
        }
        let prompt = Prompt {
            window: window.label(),
            id: session_id,
            message: sent.message,
        };
        local::send(&app, &runs, &fleet, prompt, on_event).await?;
        return Ok(session_id.to_string());
    }
    beep.post(session_id, sent, Pick::default(), on_event)
        .await?;
    Ok(session_id.to_string())
}

/// Sends the chat's `index`th prompt again on another model or thinking,
/// dropping it and the turns after it first: an admin's Rerun. An empty
/// `pick` is Retry, which anyone may use: the account's own pick.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn rerun_prompt(
    app: AppHandle,
    window: WebviewWindow,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    runs: State<'_, Runs>,
    session: String,
    index: usize,
    pick: Pick,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    center::remember_for(&window, &beep, Chat::Session(Some(session.clone())));
    if local::on(&beep, &fleet).await {
        let prompt = Prompt {
            window: window.label(),
            id: local::chat_uuid(&session)?,
            message: String::new(),
        };
        return local::rerun(&app, &runs, &fleet, index, prompt, on_event).await;
    }
    beep.rerun(session, index, pick, on_event).await
}

#[tauri::command]
pub async fn active_run(
    beep: Service<'_>,
    fleet: FleetState<'_>,
    session: String,
) -> Result<Option<String>> {
    // A reply on this Mac streams into the window that asked, and no other can join it.
    if local::on(&beep, &fleet).await {
        return Ok(None);
    }
    beep.active_run(&session).await
}

/// The chats with a run going on, started here or on another device, by
/// session id. Empty from a server too old to say.
#[tauri::command]
pub async fn active_runs(
    beep: Service<'_>,
    fleet: FleetState<'_>,
    runs: State<'_, Runs>,
) -> Result<Vec<String>> {
    if local::on(&beep, &fleet).await {
        return Ok(runs.ids().iter().map(Uuid::to_string).collect());
    }
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
pub async fn cancel_run(beep: Service<'_>, runs: State<'_, Runs>, run_id: String) -> Result<()> {
    // A reply on this Mac names its run by its chat.
    if let Ok(chat) = Uuid::parse_str(&run_id)
        && runs.ids().contains(&chat)
    {
        runs.stop(chat);
        return Ok(());
    }
    beep.call(CancelRun { id: run_id }).await
}

#[tauri::command]
pub async fn list_sessions(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
) -> Result<Vec<Session>> {
    if local::on(&beep, &fleet).await {
        return local::list(&app).await;
    }
    beep.sessions().await
}

#[tauri::command]
pub async fn search_sessions(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    query: String,
) -> Result<Vec<SessionMatch>> {
    if local::on(&beep, &fleet).await {
        return local::search(&app, &query).await;
    }
    beep.search(query).await
}

#[tauri::command]
pub async fn open_session(
    app: AppHandle,
    window: WebviewWindow,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    id: String,
) -> Result<Vec<Turn>> {
    // Made current before loading, so leaving the chat meanwhile is not undone.
    center::remember_for(&window, &beep, Chat::Session(Some(id.clone())));
    if local::on(&beep, &fleet).await {
        return local::history(&app, &id).await;
    }
    beep.history(id).await
}

/// The files other than pictures sent in a session, by the `at` of the
/// user turn they came with.
#[tauri::command]
pub async fn session_files(beep: Service<'_>, id: String) -> Result<Vec<SentFiles>> {
    // Chats kept on this Mac send no files.
    if beep.server().is_none() {
        return Ok(Vec::new());
    }
    beep.call(GetSessionFiles { session_id: id }).await
}

/// The web pages each turn of a session used, by the `at` of its user
/// turn, for the source chips under its reply.
#[tauri::command]
pub async fn session_sources(beep: Service<'_>, id: String) -> Result<Vec<TurnSources>> {
    if beep.server().is_none() {
        return Ok(Vec::new());
    }
    let session_id =
        Uuid::parse_str(&id).map_err(|_| DesktopError::BadRequest("not a chat".into()))?;
    beep.call(GetSessionSources { session_id }).await
}

/// The agent each turn of session `id` ran as, by the `at` of its user turn.
#[tauri::command]
pub async fn session_agents(beep: Service<'_>, id: String) -> Result<Vec<TurnAgent>> {
    if beep.server().is_none() {
        return Ok(Vec::new());
    }
    beep.call(GetSessionAgents { session_id: id }).await
}

#[tauri::command]
pub fn new_chat(window: WebviewWindow, beep: Service<'_>) {
    center::remember_for(&window, &beep, Chat::Session(None));
}

#[tauri::command]
pub async fn rename_session(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    id: String,
    title: String,
) -> Result<()> {
    if local::on(&beep, &fleet).await {
        return local::rename(&app, &id, &title).await;
    }
    beep.call(RenameSession { id, title }).await
}

#[tauri::command]
pub async fn pin_session(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    id: String,
    pinned: bool,
) -> Result<()> {
    if local::on(&beep, &fleet).await {
        return local::pin(&app, &id, pinned).await;
    }
    beep.call(PinSession { id, pinned }).await
}

#[tauri::command]
pub async fn delete_session(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    runs: State<'_, Runs>,
    shown: State<'_, Shown>,
    id: String,
) -> Result<()> {
    if local::on(&beep, &fleet).await {
        local::delete(&app, &runs, &id).await?;
    } else {
        beep.call(DeleteSession { id: id.clone() }).await?;
        if let Ok(chat) = Uuid::parse_str(&id) {
            beep.chats().forget(chat);
        }
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
