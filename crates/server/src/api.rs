//! Request handlers, one function per request type.

use crate::{
    app::Message,
    live::Subscription,
    protocol::*,
    router::{Cx, Router, Sink},
    store::CreateRun,
};
use chrono::Utc;
use flashmind::{
    core::{ConversationEntry, EntryKind},
    cron::CronSchedule,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    mem,
    time::Duration,
};
use tokio::{sync::broadcast::error::RecvError, time::timeout};

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const EVENTS_WINDOW: Duration = Duration::from_secs(30);

pub fn router() -> Router {
    Router::default()
        .call(auth)
        .call(health)
        .stream(chat)
        .call(get_run)
        .call(cancel_run)
        .stream(get_events)
        .call(device_result)
        .call(list_sessions)
        .call(get_session)
        .call(rename_session)
        .call(delete_session)
        .call(pin_session)
        .call(list_memories)
        .call(delete_memory)
        .call(list_crons)
        .call(create_cron)
        .call(toggle_cron)
        .call(delete_cron)
        .call(update_cron)
        .call(register_device)
        .call(unregister_device)
        .call(update_location)
        .call(save_notifications)
        .stream(stream_notifications)
        .call(list_integrations)
        .call(create_integration)
        .call(delete_integration)
        .call(update_credentials)
        .call(list_apps)
        .call(connect_app)
        .call(disconnect_app)
        .call(suggest_prompts)
}

type Reply<T> = Result<T, ApiError>;

/// Auth is handled when the connection opens; a second one is a mistake.
async fn auth(_: Cx, _: Auth) -> Reply<()> {
    Err(ApiError::bad_request("already authenticated"))
}

async fn health(_: Cx, _: Health) -> Reply<()> {
    Ok(())
}

async fn chat(cx: Cx, req: Chat, sink: Sink<Event>) -> Reply<()> {
    if req.message.trim().is_empty() || req.message.len() > 32768 {
        return Err(ApiError::bad_request("message must be 1-32768 bytes"));
    }
    let message = Message {
        session_id: req.session_id,
        request_id: req.request_id,
        message: req.message,
        cron_id: None,
        image: req.image,
    };
    match cx.app.submit(&cx.user, message).await? {
        CreateRun::Created(run) | CreateRun::Existing(run) => follow(&cx, run, 0, &sink).await,
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}

async fn get_run(cx: Cx, GetRun { id }: GetRun) -> Reply<Run> {
    owned_run(&cx, &id).await
}

async fn cancel_run(cx: Cx, CancelRun { id }: CancelRun) -> Reply<()> {
    owned_run(&cx, &id).await?;
    cx.app.cancel(&id).await;
    Ok(())
}

/// Resumes a run's events after `after`, the last `seq` the client saw.
async fn get_events(cx: Cx, req: GetEvents, sink: Sink<Event>) -> Reply<()> {
    let GetEvents { run_id, after } = req;
    if after < 0 {
        return Err(ApiError::bad_request("after must be >= 0"));
    }
    let run = owned_run(&cx, &run_id).await?;
    follow(&cx, run, after, &sink).await
}

/// Sends the run's events after `after` and follows it until it finishes, for
/// up to `EVENTS_WINDOW`. A finished run keeps no events, only its status: the
/// turn itself is in the session history.
async fn follow(cx: &Cx, run: Run, mut after: i64, sink: &Sink<Event>) -> Reply<()> {
    let Some(live) = cx.app.live(&run.id).await else {
        let finished = Event {
            seq: after + 1,
            event: RunEvent::RunFinished { status: run.status },
        };
        sink.send(&finished).await?;
        return Ok(());
    };

    let Subscription {
        backlog,
        live: mut rx,
    } = live.subscribe(after);
    let mut pending = VecDeque::from(backlog);

    let follow = async {
        loop {
            let event = match pending.pop_front() {
                Some(event) => event,
                None => match rx.recv().await {
                    Ok(event) => event,
                    Err(RecvError::Lagged(_)) => {
                        pending.extend(live.since(after));
                        continue;
                    }
                    Err(RecvError::Closed) => return Ok(()),
                },
            };
            if event.seq <= after {
                continue;
            }

            after = event.seq;
            let finished = matches!(event.event, RunEvent::RunFinished { .. });
            sink.send(&event).await?;
            if finished {
                return Ok(());
            }
        }
    };
    timeout(EVENTS_WINDOW, follow).await.unwrap_or(Ok(()))
}

async fn device_result(cx: Cx, req: DeviceResult) -> Reply<bool> {
    owned_run(&cx, &req.run_id).await?;
    Ok(cx.app.resolve_device_query(&req.query_id, req.answer).await)
}

async fn list_sessions(cx: Cx, _: ListSessions) -> Reply<Vec<Session>> {
    Ok(cx.app.store.sessions(&cx.user).await?)
}

async fn get_session(cx: Cx, GetSession { id }: GetSession) -> Reply<Vec<HistoryEntry>> {
    let conv = cx
        .app
        .store
        .conversation(&cx.user, &id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let failures = cx.app.store.tool_failures(&cx.user, &id).await?;
    Ok(history(conv.entries(), &failures))
}

/// Tool outputs in history are cut to this many bytes.
const MAX_HISTORY_TOOL_OUTPUT: usize = 16 * 1024;

/// User and assistant messages, with each turn's tool results attached to the
/// assistant message that ends the turn. A turn with tools but no text gets an
/// empty assistant message to carry them.
fn history(entries: &[ConversationEntry], failures: &HashSet<String>) -> Vec<HistoryEntry> {
    let mut history = Vec::new();
    let mut turn = Turn::default();
    for entry in entries {
        let timestamp = entry.timestamp.timestamp_millis();
        match &entry.kind {
            EntryKind::User { content, .. } => {
                turn.finish(&mut history);
                history.push(HistoryEntry::User {
                    content: content.clone(),
                    timestamp,
                });
            }
            EntryKind::Assistant {
                content,
                tool_calls,
                ..
            } => {
                for call in tool_calls.iter().flatten() {
                    turn.names.insert(call.id.clone(), call.name.clone());
                }
                if !content.is_empty() {
                    turn.last_reply = Some(history.len());
                    history.push(HistoryEntry::Assistant {
                        content: content.clone(),
                        timestamp,
                        tools: Vec::new(),
                    });
                }
            }
            EntryKind::Tool { call_id, output } => {
                turn.last_tool = timestamp;
                turn.tools.push(HistoryTool {
                    name: turn.names.get(call_id).cloned().unwrap_or_default(),
                    call_id: call_id.clone(),
                    output: cap(output, MAX_HISTORY_TOOL_OUTPUT),
                    success: !failures.contains(call_id),
                });
            }
            _ => {}
        }
    }
    turn.finish(&mut history);
    history
}

#[derive(Default)]
struct Turn {
    names: HashMap<String, String>,
    tools: Vec<HistoryTool>,
    /// Index in the history of this turn's latest assistant message.
    last_reply: Option<usize>,
    last_tool: i64,
}

impl Turn {
    fn finish(&mut self, history: &mut Vec<HistoryEntry>) {
        let turn = mem::take(self);
        if turn.tools.is_empty() {
            return;
        }
        match turn.last_reply.and_then(|index| history.get_mut(index)) {
            Some(HistoryEntry::Assistant { tools, .. }) => *tools = turn.tools,
            _ => history.push(HistoryEntry::Assistant {
                content: String::new(),
                timestamp: turn.last_tool,
                tools: turn.tools,
            }),
        }
    }
}

/// `text` cut to at most `max` bytes on a character boundary.
fn cap(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

async fn list_apps(cx: Cx, _: ListApps) -> Reply<Vec<AppIntegration>> {
    cx.app.apps.list(&cx.user).await
}

async fn connect_app(cx: Cx, ConnectApp { slug }: ConnectApp) -> Reply<ConnectLink> {
    cx.app.apps.connect(&cx.user, &slug).await
}

async fn disconnect_app(cx: Cx, DisconnectApp { slug }: DisconnectApp) -> Reply<()> {
    cx.app.apps.disconnect(&cx.user, &slug).await
}

async fn suggest_prompts(cx: Cx, _: SuggestPrompts) -> Reply<Vec<PromptSuggestion>> {
    Ok(cx.app.suggest_prompts(&cx.user).await)
}

async fn rename_session(cx: Cx, RenameSession { id, title }: RenameSession) -> Reply<()> {
    if title.is_empty() {
        return Err(ApiError::bad_request("title required"));
    }
    found(cx.app.store.rename_session(&cx.user, &id, &title).await?)
}

async fn delete_session(cx: Cx, DeleteSession { id }: DeleteSession) -> Reply<()> {
    found(cx.app.store.delete_session(&cx.user, &id).await?)
}

async fn pin_session(cx: Cx, PinSession { id, pinned }: PinSession) -> Reply<()> {
    found(cx.app.store.pin_session(&cx.user, &id, pinned).await?)
}

async fn list_memories(cx: Cx, _: ListMemories) -> Reply<Vec<Memory>> {
    Ok(cx.app.store.list_memories(&cx.user).await?)
}

async fn delete_memory(cx: Cx, DeleteMemory { key }: DeleteMemory) -> Reply<()> {
    found(cx.app.store.forget(&cx.user, &key).await?)
}

async fn list_crons(cx: Cx, _: ListCrons) -> Reply<Vec<Cron>> {
    let tz = cx
        .app
        .store
        .get_timezone(&cx.user)
        .await
        .unwrap_or_else(|_| "UTC".into());
    let mut crons = cx.app.store.list_crons(&cx.user).await?;
    for c in &mut crons {
        c.next_run_at = next_run_at(&c.schedule, &tz);
    }
    Ok(crons)
}

async fn create_cron(cx: Cx, req: CreateCron) -> Reply<String> {
    cx.app
        .store
        .create_cron(&cx.user, &req.label, &req.prompt, &req.schedule)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

async fn toggle_cron(cx: Cx, ToggleCron { id }: ToggleCron) -> Reply<bool> {
    let enabled = cx.app.store.toggle_cron(&cx.user, &id).await?;
    enabled.ok_or(ApiError::NotFound {})
}

async fn delete_cron(cx: Cx, DeleteCron { id }: DeleteCron) -> Reply<()> {
    found(cx.app.store.delete_cron(&cx.user, &id).await?)
}

async fn update_cron(cx: Cx, req: UpdateCron) -> Reply<()> {
    let label = req.label.as_deref();
    let schedule = req.schedule.as_deref();
    let updated = cx
        .app
        .store
        .update_cron(&cx.user, &req.id, req.enabled, label, schedule)
        .await?;
    found(updated)
}

async fn register_device(cx: Cx, req: RegisterDevice) -> Reply<()> {
    let store = &cx.app.store;
    store
        .register_device(&cx.user, &req.device_id, &req.fcm_token)
        .await?;
    Ok(())
}

async fn unregister_device(cx: Cx, req: UnregisterDevice) -> Reply<()> {
    found(
        cx.app
            .store
            .unregister_device(&cx.user, &req.device_id)
            .await?,
    )
}

async fn update_location(cx: Cx, req: UpdateLocation) -> Reply<()> {
    let UpdateLocation {
        latitude,
        longitude,
        accuracy,
        timezone,
    } = req;
    let valid = latitude.is_finite()
        && longitude.is_finite()
        && accuracy.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude);
    if !valid {
        return Err(ApiError::bad_request("invalid coordinates"));
    }
    let store = &cx.app.store;
    store
        .update_location_tz(&cx.user, latitude, longitude, accuracy, &timezone)
        .await?;
    Ok(())
}

async fn save_notifications(cx: Cx, req: SaveNotifications) -> Reply<u64> {
    let store = &cx.app.store;
    let count = store
        .save_notifications(&cx.user, req.notifications)
        .await?;
    Ok(count as u64)
}

/// Pushes the user's notices until the connection closes.
async fn stream_notifications(cx: Cx, _: StreamNotifications, sink: Sink<Notice>) -> Reply<()> {
    let mut rx = cx.app.notifications.subscribe();
    loop {
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            received = rx.recv() => match received {
                Ok(event) if event.user == *cx.user => {
                    match timeout(SEND_TIMEOUT, sink.send(&event.notice)).await {
                        Ok(sent) => sent?,
                        Err(_) => return Ok(()),
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }
}

async fn list_integrations(cx: Cx, _: ListIntegrations) -> Reply<Vec<Integration>> {
    Ok(cx.app.store.list_mcp_connections(&cx.user).await?)
}

async fn create_integration(cx: Cx, req: CreateIntegration) -> Reply<String> {
    cx.app
        .store
        .create_mcp_connection(&cx.user, &req.provider, &req.label, &req.config)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

async fn delete_integration(cx: Cx, DeleteIntegration { id }: DeleteIntegration) -> Reply<()> {
    found(cx.app.store.delete_mcp_connection(&cx.user, &id).await?)
}

async fn update_credentials(cx: Cx, req: UpdateCredentials) -> Reply<()> {
    let store = &cx.app.store;
    let updated = store
        .update_mcp_credentials(&cx.user, &req.id, &req.credentials, &req.status)
        .await?;
    found(updated)
}

/// The run, if it belongs to the caller.
async fn owned_run(cx: &Cx, id: &str) -> Reply<Run> {
    let run = cx.app.store.run(&cx.user, id).await?;
    run.ok_or(ApiError::NotFound {})
}

fn found(found: bool) -> Reply<()> {
    if found {
        Ok(())
    } else {
        Err(ApiError::NotFound {})
    }
}

fn next_run_at(expression: &str, tz_name: &str) -> Option<String> {
    let schedule = CronSchedule::parse(expression).ok()?;
    let tz: chrono_tz::Tz = tz_name.parse().unwrap_or(chrono_tz::UTC);
    schedule
        .next_after_in_tz(&Utc::now(), tz)
        .map(|t| t.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::types::ToolCall;
    use serde_json::json;

    #[test]
    fn every_op_is_routed_once() {
        super::router().check().unwrap();
    }

    #[test]
    fn history_attaches_tools_to_the_turn() {
        let call = |id: &str, name: &str| ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: json!({}),
        };
        let entries = [
            ConversationEntry::user("Where is lunch?"),
            ConversationEntry::assistant_with_tool_calls("", vec![call("a", "open_maps")], None),
            ConversationEntry::tool("a", "é".repeat(MAX_HISTORY_TOOL_OUTPUT)),
            ConversationEntry::assistant("Here it is."),
            ConversationEntry::user("Remind me"),
            ConversationEntry::assistant_with_tool_calls("", vec![call("b", "cron_create")], None),
            ConversationEntry::tool("b", "failed"),
        ];
        let history = history(&entries, &HashSet::from(["b".to_owned()]));
        let [
            HistoryEntry::User { .. },
            HistoryEntry::Assistant {
                content: first,
                tools: first_tools,
                ..
            },
            HistoryEntry::User { .. },
            HistoryEntry::Assistant {
                content: second,
                tools: second_tools,
                ..
            },
        ] = history.as_slice()
        else {
            panic!("unexpected history: {history:?}");
        };
        assert_eq!(first, "Here it is.");
        assert_eq!(first_tools[0].name, "open_maps");
        assert!(first_tools[0].success);
        assert_eq!(first_tools[0].output.len(), MAX_HISTORY_TOOL_OUTPUT);
        assert!(second.is_empty());
        assert_eq!(second_tools[0].call_id, "b");
        assert!(!second_tools[0].success);
    }
}
