//! The chat commands' other data source. With no server and an OpenRouter key saved, the chat
//! page, its list, its search and its model chip read and write the chats on this Mac and run the
//! model here, where they would call the server. The window above them is the same one.

use super::{runs::Runs, stream::drive};
use crate::{
    beep::{BarEvent, Beep, Turn, epoch_ms, identity::data_dir},
    chat::{history::ToolStep, markdown},
    error::{DesktopError, Result},
    fleet::projects::desk,
};
use beep_assistant::{
    AssistantError, Chat, ChatSummary, Effort, Endpoint, FIRECRAWL_BASE, OPENROUTER_BASE, Prefs,
    Setup, default_model, delete_chat, http_client, list_chats, list_models, load_chat, load_prefs,
    pin_chat, rename_chat, save_prefs, search_chats, turns,
};
use beep_fleet::Session as MeshSession;
use beep_proto::{ApiProvider, ModelChoice, Models, ReasoningLevel, Session, SessionMatch};
use secrecy::SecretString;
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tauri::{AppHandle, Emitter, ipc::Channel};
use uuid::Uuid;

const NO_KEY: &str = "Add an OpenRouter key in Assistant first.";

fn bad(e: AssistantError) -> DesktopError {
    DesktopError::BadRequest(e.to_string())
}

/// Where this Mac keeps the chats.
fn chat_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("assistant"))
}

pub(crate) fn chat_uuid(id: &str) -> Result<Uuid> {
    Uuid::parse_str(id).map_err(|_| DesktopError::BadRequest("not a chat".into()))
}

async fn key(fleet: &Arc<MeshSession>, provider: ApiProvider) -> Option<SecretString> {
    desk(fleet).await.ok()?.api_key(provider)
}

/// Whether the chat is local: no server, and an OpenRouter key this Mac can open.
pub(crate) async fn on(beep: &Beep, fleet: &Arc<MeshSession>) -> bool {
    beep.server().is_none() && key(fleet, ApiProvider::OpenRouter).await.is_some()
}

/// Tells the windows the chat list changed. An empty id is only a list change, which leaves an
/// open chat as it is.
fn changed(app: &AppHandle) {
    if let Err(e) = app.emit("sessions-changed", "") {
        log::warn!("telling the windows the chats changed: {e}");
    }
}

fn session(chat: ChatSummary) -> Session {
    Session {
        id: chat.id.to_string(),
        title: chat.title,
        pinned: chat.pinned,
        updated_at: i64::try_from(chat.updated_ms).unwrap_or(i64::MAX),
    }
}

/// The chats, newest first.
pub(crate) async fn list(app: &AppHandle) -> Result<Vec<Session>> {
    let chats = list_chats(&chat_dir(app)?).await.map_err(bad)?;
    Ok(chats.into_iter().map(session).collect())
}

pub(crate) async fn search(app: &AppHandle, query: &str) -> Result<Vec<SessionMatch>> {
    let found = search_chats(&chat_dir(app)?, query).await.map_err(bad)?;
    Ok(found
        .into_iter()
        .map(|found| SessionMatch {
            session: session(found.chat),
            snippet: found.snippet,
            from_user: found.from_user,
        })
        .collect())
}

/// A chat's messages as the window draws them: each prompt, then what the model did and said.
/// A prompt's `at` is the chat's start plus its place, which is all the window needs of it.
pub(crate) async fn history(app: &AppHandle, id: &str) -> Result<Vec<Turn>> {
    let chat = match load_chat(&chat_dir(app)?, chat_uuid(id)?).await {
        Ok(chat) => chat,
        Err(AssistantError::Store(e)) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(bad(e)),
    };
    let mut out = Vec::new();
    for (place, turn) in turns(&chat).into_iter().enumerate() {
        let at =
            i64::try_from(chat.created_ms).unwrap_or(i64::MAX) + i64::try_from(place).unwrap_or(0);
        let answered = !turn.answer.is_empty() || !turn.steps.is_empty();
        out.push(Turn::User {
            text: turn.question,
            at,
            images: Vec::new(),
            model: chat.model.clone(),
            thinking: String::new(),
        });
        if answered {
            out.push(Turn::Assistant {
                html: markdown::untrusted_html(&turn.answer),
                tools: turn
                    .steps
                    .into_iter()
                    .map(|step| ToolStep::local(step.call_id, step.name, step.label, step.ok))
                    .collect(),
                replies: Vec::new(),
                model: chat.model.clone(),
            });
        }
    }
    Ok(out)
}

pub(crate) async fn rename(app: &AppHandle, id: &str, title: &str) -> Result<()> {
    rename_chat(&chat_dir(app)?, chat_uuid(id)?, title)
        .await
        .map_err(bad)?;
    changed(app);
    Ok(())
}

pub(crate) async fn pin(app: &AppHandle, id: &str, pinned: bool) -> Result<()> {
    pin_chat(&chat_dir(app)?, chat_uuid(id)?, pinned)
        .await
        .map_err(bad)?;
    changed(app);
    Ok(())
}

pub(crate) async fn delete(app: &AppHandle, runs: &Runs, id: &str) -> Result<()> {
    let id = chat_uuid(id)?;
    let dir = chat_dir(app)?;
    // Held while the file goes, so a reply cannot start in between and write it back.
    runs.begin(id, "delete")
        .map_err(|_| DesktopError::BadRequest("Stop the reply first.".into()))?;
    let deleted = delete_chat(&dir, id).await.map_err(bad);
    runs.finish(id);
    deleted?;
    changed(app);
    Ok(())
}

/// What a prompt needs from the window.
pub(crate) struct Prompt<'a> {
    /// The window that asked, whose closing stops the reply.
    pub window: &'a str,
    pub id: Uuid,
    pub message: String,
}

/// Runs `prompt` in its chat and streams the reply into `channel` until it ends, is stopped or
/// fails.
pub(crate) async fn send(
    app: &AppHandle,
    runs: &Runs,
    fleet: &Arc<MeshSession>,
    prompt: Prompt<'_>,
    channel: Channel<BarEvent>,
) -> Result<()> {
    let message = prompt.message.trim().to_owned();
    if message.is_empty() {
        return Err(DesktopError::BadRequest("The message is empty.".into()));
    }
    let desk = desk(fleet).await?;
    let openrouter = desk
        .api_key(ApiProvider::OpenRouter)
        .ok_or_else(|| DesktopError::BadRequest(NO_KEY.into()))?;
    let firecrawl = desk.api_key(ApiProvider::Firecrawl);
    let dir = chat_dir(app)?;
    let id = prompt.id;
    // Registered before the chat is read, so no other reply or delete can change it meanwhile.
    let cancel = runs.begin(id, prompt.window)?;
    let prefs = load_prefs(&dir).await;
    let setup = Setup {
        client: http_client(),
        openrouter: Endpoint {
            base: OPENROUTER_BASE.to_owned(),
            key: openrouter,
        },
        firecrawl: firecrawl.map(|key| Endpoint {
            base: FIRECRAWL_BASE.to_owned(),
            key,
        }),
        dir: dir.clone(),
        today: chrono::Local::now().format("%Y-%m-%d").to_string(),
        effort: prefs.effort,
        cancel,
    };
    let chat = match prepare(&setup, id, prefs).await {
        Ok(chat) => chat,
        Err(e) => {
            runs.finish(id);
            return Err(e);
        }
    };
    // One task owns the whole reply, so stopping it is cancelling its token.
    let task = tauri::async_runtime::spawn(drive(app.clone(), setup, chat, message, channel));
    let joined = task.await;
    runs.finish(id);
    changed(app);
    joined?;
    Ok(())
}

/// The chat to answer in, on the picked model: the saved one or a new one. With no model picked
/// yet the chat's own, else the default of OpenRouter's list, is used and remembered.
async fn prepare(setup: &Setup, id: Uuid, mut prefs: Prefs) -> Result<Chat> {
    let mut chat = match load_chat(&setup.dir, id).await {
        Ok(chat) => chat,
        Err(AssistantError::Store(e)) if e.kind() == ErrorKind::NotFound => {
            Chat::new(id, String::new(), epoch_ms(SystemTime::now()))
        }
        Err(e) => return Err(bad(e)),
    };
    let model = if !prefs.model.is_empty() {
        prefs.model.clone()
    } else if !chat.model.is_empty() {
        chat.model.clone()
    } else {
        let models = list_models(&setup.client, &setup.openrouter)
            .await
            .map_err(bad)?;
        let last = latest_model(&setup.dir).await;
        default_model(&models, last.as_deref())
            .ok_or_else(|| DesktopError::BadRequest("OpenRouter lists no models.".into()))?
    };
    if prefs.model.is_empty() {
        prefs.model = model.clone();
        save_prefs(&setup.dir, &prefs).await.map_err(bad)?;
    }
    chat.model = model;
    Ok(chat)
}

/// The model of the chat used last.
async fn latest_model(dir: &Path) -> Option<String> {
    let chats = list_chats(dir).await.ok()?;
    chats.into_iter().next().map(|chat| chat.model)
}

/// Sends the chat's `index`th prompt again, after taking it and what followed back.
pub(crate) async fn rerun(
    app: &AppHandle,
    runs: &Runs,
    fleet: &Arc<MeshSession>,
    index: usize,
    prompt: Prompt<'_>,
    channel: Channel<BarEvent>,
) -> Result<()> {
    let dir = chat_dir(app)?;
    let mut chat = load_chat(&dir, prompt.id).await.map_err(bad)?;
    let Some(message) = chat.rewind(index) else {
        return Err(DesktopError::BadRequest(
            "that prompt is no longer in the chat".into(),
        ));
    };
    // Not while a reply is writing to the same file.
    runs.begin(prompt.id, "rerun")?;
    let saved = beep_assistant::save_chat(&dir, &chat).await.map_err(bad);
    runs.finish(prompt.id);
    saved?;
    send(app, runs, fleet, Prompt { message, ..prompt }, channel).await
}

// ---------- The model chip ----------

/// What the model chip changes.
pub(crate) enum Change {
    Nothing,
    Model(String),
    Effort(Effort),
}

/// The effort a chip level stands for.
pub(crate) fn effort(level: ReasoningLevel) -> Effort {
    match level {
        ReasoningLevel::Off => Effort::Off,
        ReasoningLevel::Low => Effort::Low,
        ReasoningLevel::Medium => Effort::Medium,
        ReasoningLevel::High => Effort::High,
    }
}

fn level(effort: Effort) -> ReasoningLevel {
    match effort {
        Effort::Off => ReasoningLevel::Off,
        Effort::Low => ReasoningLevel::Low,
        Effort::Medium => ReasoningLevel::Medium,
        Effort::High => ReasoningLevel::High,
    }
}

/// OpenRouter's models as the chip lists them, after `change`.
pub(crate) async fn models(
    app: &AppHandle,
    fleet: &Arc<MeshSession>,
    change: Change,
) -> Result<Models> {
    let dir = chat_dir(app)?;
    let endpoint = Endpoint {
        base: OPENROUTER_BASE.to_owned(),
        key: key(fleet, ApiProvider::OpenRouter)
            .await
            .ok_or_else(|| DesktopError::BadRequest(NO_KEY.into()))?,
    };
    let listed = list_models(&http_client(), &endpoint).await.map_err(bad)?;
    let mut prefs = load_prefs(&dir).await;
    let before = prefs.clone();
    match change {
        Change::Nothing => {}
        Change::Model(id) => prefs.model = id,
        Change::Effort(effort) => prefs.effort = effort,
    }
    if prefs != before {
        save_prefs(&dir, &prefs).await.map_err(bad)?;
    }
    let last = latest_model(&dir).await;
    let default = default_model(&listed, last.as_deref());
    let picked = Some(&prefs.model).filter(|model| listed.iter().any(|m| m.id == **model));
    let selected = picked
        .cloned()
        .or_else(|| default.clone())
        .unwrap_or_default();
    let choices = listed
        .into_iter()
        .map(|model| ModelChoice {
            default: default.as_deref() == Some(model.id.as_str()),
            sees_images: model.images,
            input_price: model.input_per_m,
            output_price: model.output_per_m,
            name: model.name,
            id: model.id,
        })
        .collect();
    Ok(Models {
        choices,
        selected,
        thinking: level(prefs.effort),
        auto: false,
        auto_available: false,
    })
}
