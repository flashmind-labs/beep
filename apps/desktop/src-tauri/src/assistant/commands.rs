//! The Assistant page's commands. Each is thin: the keys come from the credentials in the mesh
//! (`Desk::api_key`, never sent to the web view), the rest is `beep_assistant`.

use super::{runs::Runs, work};
use crate::{
    beep::{BarEvent, epoch_ms, identity::data_dir},
    chat::markdown,
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk},
};
use beep_assistant::{
    AssistantError, Chat, ChatSummary, Endpoint, Event, FIRECRAWL_BASE, ModelInfo, OPENROUTER_BASE,
    Outcome, Setup, TurnView, WorkQuery, default_model, delete_chat, http_client, list_chats,
    list_models, load_chat, run_turn, turns,
};
use beep_proto::ApiProvider;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::PathBuf, time::SystemTime};
use tauri::{AppHandle, State, WebviewWindow, ipc::Channel};
use tokio::sync::mpsc::{self, Receiver};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Events waiting to be drawn; a full queue slows the model's stream down.
const EVENT_QUEUE: usize = 256;
const NO_KEY: &str = "Add an OpenRouter key in Assistant first.";

fn bad(e: AssistantError) -> DesktopError {
    DesktopError::BadRequest(e.to_string())
}

fn chat_id(id: &str) -> Result<Uuid> {
    Uuid::parse_str(id).map_err(|_| DesktopError::BadRequest("not a chat".into()))
}

/// Where this Mac keeps the assistant's chats.
fn chats_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("assistant"))
}

/// The saved OpenRouter key, or the reason there is none.
async fn openrouter_key(session: &FleetState<'_>) -> Result<SecretString> {
    desk(session)
        .await?
        .api_key(ApiProvider::OpenRouter)
        .ok_or_else(|| DesktopError::BadRequest(NO_KEY.into()))
}

/// The saved chats, newest first.
#[tauri::command]
pub async fn assistant_chats(app: AppHandle) -> Result<Vec<ChatSummary>> {
    list_chats(&chats_dir(&app)?).await.map_err(bad)
}

/// A saved chat as the page draws it. The answers are HTML from `markdown::untrusted_html`, so
/// nothing in a model's reply runs or loads.
#[derive(Debug, Serialize)]
pub struct OpenChat {
    id: Uuid,
    title: String,
    model: String,
    running: bool,
    turns: Vec<TurnOut>,
}

#[derive(Debug, Serialize)]
struct TurnOut {
    #[serde(flatten)]
    turn: TurnView,
    answer_html: String,
}

#[tauri::command]
pub async fn assistant_open(app: AppHandle, runs: State<'_, Runs>, id: String) -> Result<OpenChat> {
    let id = chat_id(&id)?;
    let chat = load_chat(&chats_dir(&app)?, id).await.map_err(bad)?;
    let turns = turns(&chat)
        .into_iter()
        .map(|turn| TurnOut {
            answer_html: markdown::untrusted_html(&turn.answer),
            turn,
        })
        .collect();
    Ok(OpenChat {
        id,
        title: chat.title,
        model: chat.model,
        running: runs.is_running(id),
        turns,
    })
}

/// Deletes a chat's file. A chat that is answering is stopped first, by the page.
#[tauri::command]
pub async fn assistant_delete(app: AppHandle, runs: State<'_, Runs>, id: String) -> Result<()> {
    let id = chat_id(&id)?;
    if runs.is_running(id) {
        return Err(DesktopError::BadRequest("Stop the reply first.".into()));
    }
    delete_chat(&chats_dir(&app)?, id).await.map_err(bad)
}

/// Stops a chat's reply. What streamed so far is kept.
#[tauri::command]
pub fn assistant_stop(runs: State<'_, Runs>, id: String) -> Result<()> {
    runs.stop(chat_id(&id)?);
    Ok(())
}

/// The models OpenRouter offers and the one a new chat starts on.
#[derive(Debug, Serialize)]
pub struct ModelList {
    models: Vec<ModelInfo>,
    default: Option<String>,
}

#[tauri::command]
pub async fn assistant_models(app: AppHandle, session: FleetState<'_>) -> Result<ModelList> {
    let endpoint = Endpoint {
        base: OPENROUTER_BASE.to_owned(),
        key: openrouter_key(&session).await?,
    };
    let models = list_models(&http_client(), &endpoint).await.map_err(bad)?;
    // The model of the chat used last, while it is still listed.
    let last = list_chats(&chats_dir(&app)?)
        .await
        .map_err(bad)?
        .into_iter()
        .next()
        .map(|chat| chat.model);
    let default = default_model(&models, last.as_deref());
    Ok(ModelList { models, default })
}

/// What the page sends with a prompt.
#[derive(Debug, Deserialize)]
pub struct Prompt {
    /// The chat's id, made by the page for a new one.
    id: String,
    /// The OpenRouter model id the chat runs on from now.
    model: String,
    message: String,
    /// The user's date, `2026-11-05`.
    today: String,
}

/// Sends a prompt and streams the reply into `on_event` until it ends, is stopped, or fails.
#[tauri::command]
pub async fn assistant_send(
    app: AppHandle,
    window: WebviewWindow,
    runs: State<'_, Runs>,
    session: FleetState<'_>,
    prompt: Prompt,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    let id = chat_id(&prompt.id)?;
    let message = prompt.message.trim().to_owned();
    if message.is_empty() {
        return Err(DesktopError::BadRequest("The message is empty.".into()));
    }
    let key = openrouter_key(&session).await?;
    let firecrawl = desk(&session).await?.api_key(ApiProvider::Firecrawl);
    let dir = chats_dir(&app)?;
    let mut chat = match load_chat(&dir, id).await {
        Ok(chat) => chat,
        Err(AssistantError::Store(e)) if e.kind() == ErrorKind::NotFound => {
            let now = epoch_ms(SystemTime::now());
            Chat {
                id,
                title: String::new(),
                model: String::new(),
                created_ms: now,
                updated_ms: now,
                messages: Vec::new(),
            }
        }
        Err(e) => return Err(bad(e)),
    };
    if !prompt.model.trim().is_empty() {
        chat.model = prompt.model.trim().to_owned();
    }
    if chat.model.is_empty() {
        return Err(DesktopError::BadRequest("Pick a model first.".into()));
    }
    let cancel = runs.begin(id, window.label())?;
    let setup = Setup {
        client: http_client(),
        openrouter: Endpoint {
            base: OPENROUTER_BASE.to_owned(),
            key,
        },
        firecrawl: firecrawl.map(|key| Endpoint {
            base: FIRECRAWL_BASE.to_owned(),
            key,
        }),
        dir,
        today: prompt
            .today
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '-')
            .take(10)
            .collect(),
        cancel,
    };
    // One task owns the whole reply, so stopping it is cancelling its token.
    let task = tauri::async_runtime::spawn(drive(app, setup, chat, message, on_event));
    let joined = task.await;
    runs.finish(id);
    joined?;
    Ok(())
}

/// Runs the prompt and draws what comes, then says how it ended.
async fn drive(
    app: AppHandle,
    setup: Setup,
    mut chat: Chat,
    message: String,
    channel: Channel<BarEvent>,
) {
    let id = chat.id.to_string();
    let started = BarEvent::Started {
        run_id: id.clone(),
        session_id: id,
    };
    if channel.send(started).is_err() {
        return;
    }
    let (tx, rx) = mpsc::channel(EVENT_QUEUE);
    let cancel = setup.cancel.clone();
    let ask = async |query: WorkQuery| work::answer(&app, query).await;
    let run = async {
        let outcome = run_turn(&setup, &mut chat, message, &tx, ask).await;
        // Closing the queue ends `forward`.
        drop(tx);
        outcome
    };
    let (outcome, ()) = tokio::join!(run, forward(rx, &channel, &cancel));
    let status = match outcome {
        Ok(Outcome::Done) => "completed",
        Ok(Outcome::Stopped) => "cancelled",
        Err(e) => {
            // A closed window cannot be told; its reply is saved either way.
            channel
                .send(BarEvent::Error {
                    message: e.to_string(),
                })
                .ok();
            "failed"
        }
    };
    channel
        .send(BarEvent::Finished {
            status: status.into(),
        })
        .ok();
}

/// Sends the window what the run produces. The reply goes as the whole text so far, as HTML,
/// batched so a fast stream is not rendered once per word. A window that stops taking events
/// cancels the run.
async fn forward(mut rx: Receiver<Event>, channel: &Channel<BarEvent>, cancel: &CancellationToken) {
    let mut reply = String::new();
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }
        let mut delivered = true;
        let mut dirty = false;
        for event in batch {
            if dirty && !matches!(event, Event::Text(_)) {
                delivered &= channel.send(reply_event(&reply)).is_ok();
                dirty = false;
            }
            match event {
                Event::Text(more) => {
                    reply.push_str(&more);
                    dirty = true;
                }
                Event::Reasoning(delta) => {
                    delivered &= channel.send(BarEvent::Reasoning { delta }).is_ok();
                }
                Event::ToolStarted {
                    call_id,
                    name,
                    label,
                } => {
                    // The next round's text starts a new paragraph.
                    if !reply.is_empty() && !reply.ends_with("\n\n") {
                        reply.push_str("\n\n");
                    }
                    let started = BarEvent::ToolStarted {
                        call_id,
                        name,
                        description: label,
                    };
                    delivered &= channel.send(started).is_ok();
                }
                Event::ToolResult { call_id, name, ok } => {
                    delivered &= channel.send(tool_result(call_id, name, ok)).is_ok();
                }
            }
        }
        if dirty {
            delivered &= channel.send(reply_event(&reply)).is_ok();
        }
        if !delivered {
            cancel.cancel();
        }
    }
}

fn reply_event(reply: &str) -> BarEvent {
    BarEvent::Text {
        html: markdown::untrusted_html(reply),
    }
}

fn tool_result(call_id: String, name: String, success: bool) -> BarEvent {
    BarEvent::ToolResult {
        call_id,
        name,
        success,
        file: None,
        images: Vec::new(),
        products: None,
        cards: None,
        chart: None,
        code_agent: None,
    }
}
