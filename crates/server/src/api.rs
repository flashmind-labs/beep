//! Request handlers, one function per request type.

use crate::{
    app::{Answer, Message},
    apps::catalog_app,
    cron, feeds,
    images::{MAX_IMAGE_BYTES, MAX_IMAGES},
    live::Subscription,
    permissions,
    protocol::*,
    router::{Cx, Router, Sink},
    store::{CreateRun, Feed, Location, decode_schedule},
};
use chrono::{DateTime, Utc};
use flashmind::{
    core::{ConversationEntry, EntryKind},
    cron::{CronSchedule, JobSchedule},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    mem,
    time::Duration,
};
use tokio::{sync::broadcast::error::RecvError, time::timeout};
use uuid::Uuid;

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const EVENTS_WINDOW: Duration = Duration::from_secs(30);

pub fn router() -> Router {
    Router::default()
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
        .call(delete_sessions)
        .call(pin_session)
        .call(list_memories)
        .call(delete_memory)
        .call(update_memory)
        .call(clear_memories)
        .call(list_crons)
        .call(create_cron)
        .call(toggle_cron)
        .call(delete_cron)
        .call(update_cron)
        .call(run_cron)
        .call(register_device)
        .call(unregister_device)
        .call(update_location)
        .call(save_notifications)
        .stream(stream_notifications)
        .call(list_apps)
        .call(connect_app)
        .call(disconnect_app)
        .call(start_app_auth)
        .call(finish_app_auth)
        .call(suggest_prompts)
        .call(get_app_permissions)
        .call(set_app_permissions)
        .call(answer_approval)
        .call(get_models)
        .call(set_model)
        .call(existing_sessions)
}

type Reply<T> = Result<T, ApiError>;

async fn health(_: Cx, _: Health) -> Reply<()> {
    Ok(())
}

async fn chat(cx: Cx, req: Chat, sink: Sink<Event>) -> Reply<()> {
    if req.message.trim().is_empty() || req.message.len() > 32768 {
        return Err(ApiError::bad_request("message must be 1-32768 bytes"));
    }
    if req.images.len() > MAX_IMAGES {
        return Err(ApiError::bad_request("at most 8 images per message"));
    }
    if req.images.iter().any(|image| image.len() > MAX_IMAGE_BYTES) {
        return Err(ApiError::bad_request("each image must be at most 2 MiB"));
    }
    let model = match &req.model {
        Some(id) => {
            if !cx.app.models.is_admin(cx.user.id()) {
                return Err(ApiError::Unauthorized {});
            }
            Some(cx.app.models.find(id).ok_or(ApiError::NotFound {})?.clone())
        }
        None => None,
    };
    let message = Message {
        session_id: req.session_id,
        request_id: req.request_id,
        message: req.message,
        cron_id: None,
        images: req.images,
        foreground: req.foreground,
        answer: None,
        model,
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
    Ok(cx.user.sessions().await?)
}

async fn get_session(cx: Cx, GetSession { id }: GetSession) -> Reply<Vec<HistoryEntry>> {
    let conv = cx
        .user
        .conversation(&id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let failures = cx.user.tool_failures(&id).await?;
    let scores = cx.user.memory_scores(&id).await?;
    Ok(history(conv.entries(), &failures, &scores))
}

/// Tool outputs in history are cut to this many bytes.
const MAX_HISTORY_TOOL_OUTPUT: usize = 16 * 1024;
/// A turn's reasoning in history keeps its last this many bytes.
const MAX_HISTORY_REASONING: usize = 16 * 1024;

/// User and assistant messages, with each turn's tool results attached to the
/// assistant message that ends the turn. A turn with tools but no text gets an
/// empty assistant message to carry them. `scores` holds Jev's memory scores
/// by the millisecond timestamp of the user message that started the turn.
fn history(
    entries: &[ConversationEntry],
    failures: &HashSet<String>,
    scores: &HashMap<i64, f64>,
) -> Vec<HistoryEntry> {
    let mut history = Vec::new();
    let mut turn = Turn::default();
    for entry in entries {
        let timestamp = entry.timestamp.timestamp_millis();
        match &entry.kind {
            EntryKind::User { content, .. } => {
                turn.finish(&mut history);
                turn.score = scores.get(&timestamp).copied();
                history.push(match cron::scheduled_label(content) {
                    Some(label) => HistoryEntry::Scheduled {
                        label: label.to_owned(),
                        timestamp,
                    },
                    None => HistoryEntry::User {
                        content: content.clone(),
                        timestamp,
                    },
                });
            }
            EntryKind::Assistant {
                content,
                tool_calls,
                reasoning,
            } => {
                if let Some(reasoning) = reasoning.as_deref().map(str::trim)
                    && !reasoning.is_empty()
                {
                    if !turn.reasoning.is_empty() {
                        turn.reasoning.push_str("\n\n");
                    }
                    turn.reasoning.push_str(reasoning);
                }
                for call in tool_calls.iter().flatten() {
                    turn.names.insert(call.id.clone(), call.name.clone());
                }
                if !content.is_empty() {
                    turn.last_reply = Some(history.len());
                    history.push(HistoryEntry::Assistant {
                        content: content.clone(),
                        timestamp,
                        tools: Vec::new(),
                        memory_score: None,
                        reasoning: String::new(),
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
    score: Option<f64>,
    /// Every step's reasoning, oldest first.
    reasoning: String,
}

impl Turn {
    fn finish(&mut self, history: &mut Vec<HistoryEntry>) {
        let turn = mem::take(self);
        match turn.last_reply.and_then(|index| history.get_mut(index)) {
            Some(HistoryEntry::Assistant {
                tools,
                memory_score,
                reasoning,
                ..
            }) => {
                *tools = turn.tools;
                *memory_score = turn.score;
                *reasoning = cap_tail(&turn.reasoning, MAX_HISTORY_REASONING);
            }
            _ if turn.tools.is_empty() => {}
            _ => history.push(HistoryEntry::Assistant {
                content: String::new(),
                timestamp: turn.last_tool,
                tools: turn.tools,
                memory_score: turn.score,
                reasoning: cap_tail(&turn.reasoning, MAX_HISTORY_REASONING),
            }),
        }
    }
}

/// The end of `text`, at most `max` bytes on a character boundary.
fn cap_tail(text: &str, max: usize) -> String {
    let mut start = text.len().saturating_sub(max);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
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
    let apps = cx.user.apps().await?;
    // OAuth ends in the browser and the phone lists apps again on return,
    // so this is the first moment the server sees a new mailbox or Slack.
    let feeds: Vec<Feed> = apps
        .iter()
        .filter(|app| app.connected)
        .filter_map(|app| Feed::from_slug(&app.slug))
        .collect();
    if !feeds.is_empty() {
        feeds::spawn_sync(&cx.app, &cx.user, feeds);
    }
    Ok(apps)
}

async fn connect_app(cx: Cx, ConnectApp { slug }: ConnectApp) -> Reply<ConnectLink> {
    cx.user.connect_app(&slug).await
}

async fn disconnect_app(cx: Cx, DisconnectApp { slug }: DisconnectApp) -> Reply<()> {
    cx.user.disconnect_app(&slug).await
}

async fn get_models(cx: Cx, _: GetModels) -> Reply<Models> {
    let selected = cx.app.model_for(&cx.user).await;
    Ok(cx.app.models.for_picker(cx.user.id(), &selected))
}

async fn set_model(cx: Cx, SetModel { id }: SetModel) -> Reply<Models> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    let model = cx
        .app
        .models
        .find(&id)
        .ok_or(ApiError::NotFound {})?
        .clone();
    cx.user.set_model(&model).await?;
    log::info!("model picked; user={} model={model}", cx.user.id());
    Ok(cx.app.models.for_picker(cx.user.id(), &model))
}

async fn get_app_permissions(
    cx: Cx,
    GetAppPermissions { slug }: GetAppPermissions,
) -> Reply<AppPermissions> {
    let app = catalog_app(&slug).ok_or(ApiError::NotFound {})?;
    let rules = cx.user.app_rules().await?;
    Ok(permissions::app_permissions(app, &rules))
}

async fn set_app_permissions(cx: Cx, req: SetAppPermissions) -> Reply<()> {
    let app = catalog_app(&req.slug).ok_or(ApiError::NotFound {})?;
    let row = permissions::row(app, req.level, req.allowed);
    Ok(cx.user.save_app_permissions(&row).await?)
}

/// Resumes a session that stopped on a tool call to ask the user. Only the
/// first open call can be answered; answering it again returns the run the
/// first answer started.
async fn answer_approval(cx: Cx, req: AnswerApproval) -> Reply<Run> {
    let AnswerApproval {
        session_id,
        query_id,
        decision,
    } = req;
    let message = Message {
        session_id,
        request_id: Uuid::nil(),
        message: decision.as_str().to_owned(),
        cron_id: None,
        images: Vec::new(),
        foreground: None,
        answer: Some(Answer {
            call_id: query_id.clone(),
            decision,
        }),
        model: None,
    };
    let session = session_id.to_string();
    let conversation = match cx.user.context(&session).await? {
        Some(context) => Some(context.conversation),
        None => cx.user.conversation(&session).await?,
    };
    let open = conversation.as_ref().map(permissions::open_calls);
    let waiting = open
        .and_then(|open| open.into_iter().next())
        .is_some_and(|call| call.id == query_id);
    let created = if waiting {
        cx.app.submit(&cx.user, message).await?
    } else {
        cx.user
            .existing_run(&session, &message.request(), &message.message)
            .await?
            .ok_or_else(|| ApiError::Conflict {
                message: "nothing is waiting for this answer".into(),
            })?
    };
    match created {
        CreateRun::Created(run) | CreateRun::Existing(run) => Ok(run),
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}

async fn start_app_auth(
    cx: Cx,
    StartAppAuth { slug, redirect_uri }: StartAppAuth,
) -> Reply<ConnectLink> {
    cx.user.start_app_auth(&slug, &redirect_uri).await
}

async fn finish_app_auth(cx: Cx, req: FinishAppAuth) -> Reply<()> {
    cx.user
        .finish_app_auth(&req.slug, &req.code, &req.state, req.issuer.as_deref())
        .await
}

async fn suggest_prompts(
    cx: Cx,
    SuggestPrompts { refresh }: SuggestPrompts,
) -> Reply<PromptSuggestions> {
    Ok(cx.app.suggest_prompts(&cx.user, refresh).await)
}

async fn rename_session(cx: Cx, RenameSession { id, title }: RenameSession) -> Reply<()> {
    if title.is_empty() {
        return Err(ApiError::bad_request("title required"));
    }
    found(cx.user.rename_session(&id, &title).await?)
}

async fn delete_session(cx: Cx, DeleteSession { id }: DeleteSession) -> Reply<()> {
    found(cx.user.delete_session(&id).await?)
}

async fn delete_sessions(cx: Cx, DeleteSessions { ids }: DeleteSessions) -> Reply<u64> {
    if ids.len() > 200 {
        return Err(ApiError::bad_request("at most 200 sessions at once"));
    }
    Ok(cx.user.delete_sessions(ids).await?)
}

async fn existing_sessions(
    cx: Cx,
    ExistingSessions { ids }: ExistingSessions,
) -> Reply<Vec<String>> {
    if ids.len() > 1000 {
        return Err(ApiError::bad_request("at most 1000 sessions at once"));
    }
    Ok(cx.user.existing_sessions(ids).await?)
}

async fn pin_session(cx: Cx, PinSession { id, pinned }: PinSession) -> Reply<()> {
    found(cx.user.pin_session(&id, pinned).await?)
}

async fn list_memories(cx: Cx, _: ListMemories) -> Reply<Vec<Memory>> {
    Ok(cx.user.list_memories().await?)
}

async fn delete_memory(cx: Cx, DeleteMemory { id }: DeleteMemory) -> Reply<()> {
    found(cx.user.forget(&id).await?)
}

/// The new text is embedded on the next lookup.
async fn update_memory(cx: Cx, UpdateMemory { id, content }: UpdateMemory) -> Reply<()> {
    let content = content.trim();
    if content.is_empty() || content.len() > 4000 {
        return Err(ApiError::bad_request("memory text must be 1 to 4000 bytes"));
    }
    found(cx.user.edit_memory(&id, content).await?)
}

async fn clear_memories(cx: Cx, _: ClearMemories) -> Reply<u64> {
    Ok(cx.user.forget_all().await?)
}

async fn list_crons(cx: Cx, _: ListCrons) -> Reply<Vec<Cron>> {
    let tz = cx
        .user
        .get_timezone()
        .await
        .unwrap_or_else(|_| "UTC".into());
    // Recurring routines first, then one-off alerts, each soonest first. A
    // routine with no next run (a bad expression) goes last in its group.
    let mut crons: Vec<_> = (cx.user.list_crons().await?.into_iter())
        .map(|c| (next_run_at(&c.schedule, &tz), c))
        .collect();
    crons.sort_by_key(|(next, c)| (c.run_once, next.is_none(), *next));
    Ok(crons
        .into_iter()
        .map(|(next, mut c)| {
            c.next_run_at = next.map(|t| t.to_rfc3339());
            c
        })
        .collect())
}

async fn create_cron(cx: Cx, req: CreateCron) -> Reply<String> {
    cx.user
        .create_cron(&req.label, &req.prompt, &req.schedule)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

async fn toggle_cron(cx: Cx, ToggleCron { id }: ToggleCron) -> Reply<bool> {
    let enabled = cx.user.toggle_cron(&id).await?;
    enabled.ok_or(ApiError::NotFound {})
}

async fn delete_cron(cx: Cx, DeleteCron { id }: DeleteCron) -> Reply<()> {
    found(cx.user.delete_cron(&id).await?)
}

async fn update_cron(cx: Cx, req: UpdateCron) -> Reply<()> {
    let label = req.label.as_deref();
    let schedule = req.schedule.as_deref();
    let updated = cx
        .user
        .update_cron(&req.id, req.enabled, label, schedule)
        .await?;
    found(updated)
}

async fn run_cron(cx: Cx, RunCron { id }: RunCron) -> Reply<String> {
    let uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    let job = cx.user.cron_job(uuid).await?.ok_or(ApiError::NotFound {})?;
    let run = cron::run_job(&cx.app, &cx.user, &job)
        .await?
        .ok_or_else(|| ApiError::bad_request("the routine could not start"))?;
    // A one-off reminder that ran early is done; leaving it would run it again.
    if job.once {
        cx.user.delete_cron(&id).await?;
    }
    Ok(run.session_id)
}

async fn register_device(cx: Cx, req: RegisterDevice) -> Reply<()> {
    cx.user
        .register_device(&req.device_id, &req.fcm_token)
        .await?;
    Ok(())
}

async fn unregister_device(cx: Cx, req: UnregisterDevice) -> Reply<()> {
    found(cx.user.unregister_device(&req.device_id).await?)
}

async fn update_location(cx: Cx, req: UpdateLocation) -> Reply<()> {
    let UpdateLocation {
        latitude,
        longitude,
        accuracy,
        timezone,
        place,
    } = req;
    let valid = latitude.is_finite()
        && longitude.is_finite()
        && accuracy.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude);
    if !valid {
        return Err(ApiError::bad_request("invalid coordinates"));
    }
    let place = place
        .map(|p| p.trim().chars().take(200).collect::<String>())
        .filter(|p| !p.is_empty());
    let fix = Location {
        latitude,
        longitude,
        accuracy,
        timezone,
        place,
        updated_at: Utc::now(),
    };
    cx.user.update_location(&fix).await?;
    Ok(())
}

async fn save_notifications(cx: Cx, req: SaveNotifications) -> Reply<u64> {
    let count = cx.user.save_notifications(req.notifications).await?;
    Ok(count as u64)
}

/// Pushes the user's notices until the connection closes.
async fn stream_notifications(cx: Cx, _: StreamNotifications, sink: Sink<Notice>) -> Reply<()> {
    let mut rx = cx.app.notifications.subscribe();
    loop {
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            received = rx.recv() => match received {
                Ok(event) if event.user == cx.user.id() => {
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

/// The run, if it belongs to the caller.
async fn owned_run(cx: &Cx, id: &str) -> Reply<Run> {
    let run = cx.user.run(id).await?;
    run.ok_or(ApiError::NotFound {})
}

fn found(found: bool) -> Reply<()> {
    if found {
        Ok(())
    } else {
        Err(ApiError::NotFound {})
    }
}

fn next_run_at(schedule: &str, tz_name: &str) -> Option<DateTime<Utc>> {
    let expression = match decode_schedule(schedule.to_owned()) {
        JobSchedule::Once(at) => return Some(at),
        JobSchedule::Cron(expression) => expression,
        JobSchedule::OnWake { .. } => return None,
    };
    let tz: chrono_tz::Tz = tz_name.parse().unwrap_or(chrono_tz::UTC);
    CronSchedule::parse(&expression)
        .ok()?
        .next_after_in_tz(&Utc::now(), tz)
        .map(|t| t.with_timezone(&Utc))
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
        let mut entries = [
            ConversationEntry::user("Where is lunch?"),
            ConversationEntry::assistant_with_tool_calls(
                "",
                vec![call("a", "open_maps")],
                Some("Look it up.".into()),
            ),
            ConversationEntry::tool("a", "é".repeat(MAX_HISTORY_TOOL_OUTPUT)),
            ConversationEntry::assistant_with_reasoning("Here it is.", Some("Found one.".into())),
            ConversationEntry::user("Remind me"),
            ConversationEntry::assistant_with_tool_calls("", vec![call("b", "cron_create")], None),
            ConversationEntry::tool("b", "failed"),
        ];
        // Each turn's score is found by its user message's time, so they must differ.
        entries[4].timestamp = entries[0].timestamp + chrono::Duration::seconds(1);
        let asked = entries[0].timestamp.timestamp_millis();
        let history = history(
            &entries,
            &HashSet::from(["b".to_owned()]),
            &HashMap::from([(asked, 0.8)]),
        );
        let [
            HistoryEntry::User { .. },
            HistoryEntry::Assistant {
                content: first,
                tools: first_tools,
                memory_score: first_score,
                reasoning: first_reasoning,
                ..
            },
            HistoryEntry::User { .. },
            HistoryEntry::Assistant {
                content: second,
                tools: second_tools,
                memory_score: second_score,
                reasoning: second_reasoning,
                ..
            },
        ] = history.as_slice()
        else {
            panic!("unexpected history: {history:?}");
        };
        assert_eq!(first, "Here it is.");
        assert_eq!(*first_score, Some(0.8));
        assert_eq!(*second_score, None);
        assert_eq!(first_reasoning, "Look it up.\n\nFound one.");
        assert!(second_reasoning.is_empty());
        assert_eq!(first_tools[0].name, "open_maps");
        assert!(first_tools[0].success);
        assert_eq!(first_tools[0].output.len(), MAX_HISTORY_TOOL_OUTPUT);
        assert!(second.is_empty());
        assert_eq!(second_tools[0].call_id, "b");
        assert!(!second_tools[0].success);
    }

    #[test]
    fn history_marks_a_routine_prompt_as_scheduled() {
        let entries = [
            ConversationEntry::user("[Scheduled: Plants] Remind me to water the plants"),
            ConversationEntry::assistant("Time to water the plants."),
        ];
        let history = history(&entries, &HashSet::new(), &HashMap::new());
        let [
            HistoryEntry::Scheduled { label, .. },
            HistoryEntry::Assistant { .. },
        ] = history.as_slice()
        else {
            panic!("unexpected history: {history:?}");
        };
        assert_eq!(label, "Plants");
    }
}
