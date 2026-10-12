//! Request handlers, one function per request type.

use crate::{
    agents::AgentId,
    app::{Answer, Message, UserNotice, group_message},
    apps::catalog_app,
    cron, feeds,
    images::{self, MAX_IMAGE_BYTES, MAX_IMAGES},
    live::Subscription,
    models::Picked,
    permissions, places, profile,
    protocol::{search::match_snippet, *},
    router::{Cx, Router, Sink},
    skills, steps,
    store::{
        AccountKey, CopiedShare, CreateRun, DeletedTurn, Feed, FoundSession, Inviter, Join,
        LinkKey, Location, Merge, MergeInto, NewKey, PushDevice, SavedPlace, SkillPatch, SkillRow,
        TurnsToDelete, decode_schedule, encode_schedule,
    },
    tools::stocks::{self, StockError, Symbol},
    wake,
};
use chrono::{DateTime, SecondsFormat, Utc};
use chrono_tz::{Tz, UTC};
use flashmind::{
    core::{Conversation, ConversationEntry, EntryKind},
    cron::{CronSchedule, JobSchedule},
};
use iroh::EndpointId;
use std::{
    collections::{HashSet, VecDeque},
    time::{Duration, SystemTime},
};
use tokio::{
    sync::broadcast::error::RecvError,
    time::{sleep, timeout},
};
use uuid::Uuid;

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const EVENTS_WINDOW: Duration = Duration::from_secs(30);
/// How long a join code works.
const INVITE_TTL: Duration = Duration::from_secs(10 * 60);
/// How long the joining keys keep their connections, so the reply that
/// they joined gets there first.
const REJOIN_AFTER: Duration = Duration::from_secs(1);

pub fn router() -> Router {
    Router::default()
        .call(hello)
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
        .call(set_timezone)
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
        .call(set_thinking)
        .call(existing_sessions)
        .call(delete_turn)
        .call(delete_turns_from)
        .call(search_sessions)
        .call(set_auto_model)
        .call(get_agents)
        .call(set_agents)
        .call(link_device)
        .call(list_devices)
        .call(remove_device)
        .call(invite_device)
        .call(peek_invite)
        .call(join_account)
        .call(share_session)
        .call(unshare_session)
        .call(get_share)
        .call(copy_share)
        .call(create_group)
        .call(list_groups)
        .call(get_group)
        .call(send_to_group)
        .call(group_invite)
        .call(peek_group)
        .call(join_group)
        .call(leave_group)
        .call(pin_group)
        .call(share_group_apps)
        .call(answer_group_approval)
        .call(get_file)
        .call(list_group_reminders)
        .call(delete_group_reminder)
        .call(list_places)
        .call(delete_place)
        .call(update_place)
        .call(cross_place)
        .call(list_memory_reads)
        .call(get_session_images)
        .call(get_session_agents)
        .call(get_group_images)
        .call(get_updates)
        .call(list_skills)
        .call(update_skill)
        .call(delete_skill)
        .call(undo_skill)
        .stream(stream_updates)
        .call(stock_chart)
}

type Reply<T> = Result<T, ApiError>;

/// The transport reads the `Hello` that opens a connection; a later one
/// changes nothing.
async fn hello(_: Cx, _: Hello) -> Reply<()> {
    Ok(())
}

async fn health(_: Cx, _: Health) -> Reply<()> {
    Ok(())
}

async fn chat(cx: Cx, req: Chat, sink: Sink<Event>) -> Reply<()> {
    if req.message.trim().is_empty() || req.message.len() > 32768 {
        return Err(ApiError::bad_request("message must be 1-32768 bytes"));
    }
    check_images(&req.images)?;
    if (req.model.is_some() || req.thinking.is_some()) && !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    let model = match &req.model {
        Some(id) => Some(cx.app.models.find(id).ok_or(ApiError::NotFound {})?.clone()),
        None => None,
    };
    let message = Message {
        session_id: req.session_id,
        request_id: req.request_id,
        message: req.message,
        cron_id: None,
        agent: None,
        scheduled: None,
        woken: None,
        images: req.images,
        answer: None,
        model,
        thinking: req.thinking,
        origin: cx.device,
        platform: req.platform,
        charts: draws_charts(&cx),
    };
    match cx.app.submit(&cx.user, message).await? {
        CreateRun::Created(run) | CreateRun::Existing(run) => follow(&cx, run, 0, &sink).await,
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}

/// Whether a run started on this connection gets the chart tools: only the
/// macOS app draws them.
fn draws_charts(cx: &Cx) -> bool {
    cx.device == (DeviceKind::Desktop {}) && cx.supports(Feature::CHARTS)
}

/// One symbol's chart over another range, for a stock chart's range buttons.
async fn stock_chart(_: Cx, req: StockChart) -> Reply<Chart> {
    let symbol = Symbol::parse(&req.symbol).map_err(|e| ApiError::bad_request(e.to_string()))?;
    match stocks::fetch(&[symbol], req.range).await {
        Ok(histories) => Ok(stocks::chart(req.range, &histories)),
        Err(StockError::NotFound(_) | StockError::NoPrices(_)) => Err(ApiError::NotFound {}),
        Err(e) => {
            log::warn!("stock chart failed: {e}; symbol={}", req.symbol);
            Err(ApiError::Internal {})
        }
    }
}

/// Refuses too many images, or one too large.
fn check_images(images: &[String]) -> Reply<()> {
    if images.len() > MAX_IMAGES {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_IMAGES} images per message"
        )));
    }
    if images.iter().any(|image| image.len() > MAX_IMAGE_BYTES) {
        return Err(ApiError::bad_request(format!(
            "each image must be at most {} MiB",
            MAX_IMAGE_BYTES >> 20
        )));
    }
    Ok(())
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
            event: RunEvent::RunFinished {
                status: run.status,
                updated_at: None,
            },
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
    let messages = cx
        .user
        .session_messages(chat_id(&id)?)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(steps::history(&messages))
}

async fn share_session(cx: Cx, ShareSession { id }: ShareSession) -> Reply<SharedLink> {
    let id = chat_id(&id)?;
    let messages = cx
        .user
        .session_messages(id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let entries = steps::history(&messages);
    let share = cx
        .user
        .share_session(id, &shared_conversation(&entries))
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(SharedLink {
        url: share_url(&share.token),
        token: share.token,
        title: share.title,
        shared_at: share.shared_at.timestamp_millis(),
    })
}

async fn unshare_session(cx: Cx, UnshareSession { id }: UnshareSession) -> Reply<()> {
    cx.user.unshare_session(chat_id(&id)?).await?;
    Ok(())
}

async fn get_share(cx: Cx, GetShare { token }: GetShare) -> Reply<SharedSession> {
    let share = cx
        .app
        .store
        .share(&token)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(SharedSession {
        entries: share_history(&share.conversation),
        title: share.title,
        shared_at: share.shared_at.timestamp_millis(),
    })
}

async fn copy_share(cx: Cx, CopyShare { token, session_id }: CopyShare) -> Reply<Session> {
    let id = session_id.to_string();
    match cx.user.copy_share(&token, session_id).await? {
        CopiedShare::Copied => {}
        CopiedShare::NotFound => return Err(ApiError::NotFound {}),
        CopiedShare::Taken => {
            return Err(ApiError::Conflict {
                message: "session id is taken".into(),
            });
        }
    }
    cx.user
        .sessions()
        .await?
        .into_iter()
        .find(|session| session.id == id)
        .ok_or(ApiError::Internal {})
}

/// Messages a group chat loads at once.
const GROUP_MESSAGES: i64 = 200;

/// A group's name, trimmed.
fn group_name(name: &str) -> Reply<String> {
    trimmed_name(name, "group name", 60)
}

/// What a member goes by in a group, trimmed. Nobody may go by Beep.
fn group_member_name(name: &str) -> Reply<String> {
    let name = trimmed_name(name, "member name", 30)?;
    if name.eq_ignore_ascii_case("beep") {
        return Err(ApiError::bad_request("Beep is taken"));
    }
    Ok(name)
}

fn trimmed_name(name: &str, what: &str, max: usize) -> Reply<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > max {
        return Err(ApiError::bad_request(format!(
            "{what} must be 1-{max} characters"
        )));
    }
    Ok(name.to_owned())
}

async fn create_group(
    cx: Cx,
    CreateGroup {
        id,
        name,
        member_name,
    }: CreateGroup,
) -> Reply<Group> {
    let name = group_name(&name)?;
    let member_name = group_member_name(&member_name)?;
    if !cx.user.create_group(id, &name, &member_name).await? {
        return Err(ApiError::Conflict {
            message: "group id is taken".into(),
        });
    }
    let joined = cx.user.group(id).await?.ok_or(ApiError::Internal {})?;
    Ok(cx.app.group_view(&joined, cx.user.id()))
}

async fn list_groups(cx: Cx, _: ListGroups) -> Reply<Vec<Group>> {
    Ok(cx
        .user
        .groups()
        .await?
        .iter()
        .map(|joined| cx.app.group_view(joined, cx.user.id()))
        .collect())
}

async fn get_group(cx: Cx, GetGroup { id }: GetGroup) -> Reply<GroupChat> {
    let id = chat_id(&id)?;
    let joined = cx.user.group(id).await?.ok_or(ApiError::NotFound {})?;
    let messages = cx.app.store.group_messages(id, GROUP_MESSAGES).await?;
    Ok(GroupChat {
        group: cx.app.group_view(&joined, cx.user.id()),
        messages: messages
            .iter()
            .map(|row| group_message(row, cx.user.id()))
            .collect(),
    })
}

async fn send_to_group(
    cx: Cx,
    SendToGroup {
        group_id,
        request_id,
        message,
        images,
    }: SendToGroup,
) -> Reply<GroupMessage> {
    if message.trim().is_empty() || message.len() > 8192 {
        return Err(ApiError::bad_request("message must be 1-8192 bytes"));
    }
    check_images(&images)?;
    let pictures = images::pictures(&images);
    cx.app
        .send_to_group(
            &cx.user,
            chat_id(&group_id)?,
            request_id,
            message.trim(),
            &pictures,
        )
        .await?
        .ok_or(ApiError::NotFound {})
}

async fn get_session_images(
    cx: Cx,
    GetSessionImages { session_id }: GetSessionImages,
) -> Reply<Vec<SentImages>> {
    Ok(cx.user.session_pictures(chat_id(&session_id)?).await?)
}

async fn get_session_agents(
    cx: Cx,
    GetSessionAgents { session_id }: GetSessionAgents,
) -> Reply<Vec<TurnAgent>> {
    let turns = cx
        .user
        .turn_agents(chat_id(&session_id)?)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let agents = cx.app.models.agents();
    Ok(turns
        .into_iter()
        .map(|turn| TurnAgent {
            message: turn.sent_at,
            agent: turn
                .agent
                .filter(|&id| id != AgentId::General)
                .and_then(|id| agents.iter().find(|agent| agent.id == id))
                .map(|agent| agent.name.clone()),
        })
        .collect())
}

async fn get_group_images(
    cx: Cx,
    GetGroupImages { group_id }: GetGroupImages,
) -> Reply<Vec<SentImages>> {
    let group_id = chat_id(&group_id)?;
    if cx.user.group(group_id).await?.is_none() {
        return Err(ApiError::NotFound {});
    }
    Ok(cx.app.store.group_pictures(group_id).await?)
}

async fn group_invite(cx: Cx, GroupInvite { id, reset }: GroupInvite) -> Reply<GroupLink> {
    let token = cx
        .user
        .group_invite(chat_id(&id)?, reset)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(GroupLink {
        url: group_url(&token),
        token,
    })
}

async fn peek_group(cx: Cx, PeekGroup { token }: PeekGroup) -> Reply<GroupPreview> {
    let token = group_code(&token).ok_or(ApiError::NotFound {})?;
    let joined = cx
        .app
        .store
        .group_by_invite(&token)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(GroupPreview {
        joined: joined.members.iter().any(|m| m.user_id == cx.user.id()),
        members: joined.members.into_iter().map(|m| m.name).collect(),
        id: joined.group.id.to_string(),
        name: joined.group.name,
    })
}

async fn join_group(cx: Cx, JoinGroup { token, member_name }: JoinGroup) -> Reply<Group> {
    let member_name = group_member_name(&member_name)?;
    let token = group_code(&token).ok_or(ApiError::NotFound {})?;
    let invited = cx
        .app
        .store
        .group_by_invite(&token)
        .await?
        .ok_or(ApiError::NotFound {})?;
    // Beep tells members apart by name, so two may not share one.
    let taken = invited
        .members
        .iter()
        .any(|m| m.user_id != cx.user.id() && m.name.eq_ignore_ascii_case(&member_name));
    if taken {
        return Err(ApiError::Conflict {
            message: "someone in the group already goes by that name".into(),
        });
    }
    let id = match cx.user.join_group(&token, &member_name).await? {
        Join::Joined(id) => id,
        Join::NameTaken => {
            return Err(ApiError::Conflict {
                message: "someone in the group already goes by that name".into(),
            });
        }
        Join::UnknownInvite => return Err(ApiError::NotFound {}),
    };
    let joined = cx.user.group(id).await?.ok_or(ApiError::Internal {})?;
    for member in &joined.members {
        let _ = cx
            .app
            .notifications
            .send(UserNotice::group_changed(&member.user_id, &id.to_string()));
    }
    Ok(cx.app.group_view(&joined, cx.user.id()))
}

async fn pin_group(cx: Cx, PinGroup { id, pinned }: PinGroup) -> Reply<()> {
    found(cx.user.pin_group(chat_id(&id)?, pinned).await?)
}

async fn get_file(cx: Cx, GetFile { id, offset }: GetFile) -> Reply<FilePart> {
    cx.user
        .file_part(&id, offset, FILE_PART)
        .await?
        .ok_or(ApiError::NotFound {})
}

async fn share_group_apps(cx: Cx, ShareGroupApps { id, shared }: ShareGroupApps) -> Reply<()> {
    found(cx.user.share_group_apps(chat_id(&id)?, shared).await?)
}

async fn list_group_reminders(
    cx: Cx,
    ListGroupReminders { group_id }: ListGroupReminders,
) -> Reply<Vec<GroupReminder>> {
    let group_id = chat_id(&group_id)?;
    let joined = cx
        .user
        .group(group_id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let fallback = cx
        .user
        .get_timezone()
        .await
        .unwrap_or_else(|_| "UTC".into());
    let mut reminders = Vec::new();
    for job in cx.app.store.group_reminders(group_id).await? {
        if !job.enabled {
            continue;
        }
        let setter = job.metadata.get("user_id").and_then(|v| v.as_str());
        let member = joined
            .members
            .iter()
            .find(|m| Some(m.user_id.as_str()) == setter);
        // A cron expression is in the setter's time, as the runner reads it.
        let tz = match setter.and_then(|id| cx.app.user(id)) {
            Some(user) => user
                .get_timezone()
                .await
                .unwrap_or_else(|_| fallback.clone()),
            None => fallback.clone(),
        };
        let next = next_run(&job.schedule, &tz);
        reminders.push((
            next,
            GroupReminder {
                id: job.id.to_string(),
                label: (job.metadata.get("label").and_then(|v| v.as_str()))
                    .unwrap_or("Reminder")
                    .to_owned(),
                message: job.task,
                schedule: encode_schedule(&job.schedule),
                next_run_at: next.map(|t| t.to_rfc3339()),
                set_by: member.map_or_else(|| "A former member".to_owned(), |m| m.name.clone()),
            },
        ));
    }
    reminders.sort_by_key(|(next, _)| (next.is_none(), *next));
    Ok(reminders.into_iter().map(|(_, r)| r).collect())
}

async fn delete_group_reminder(
    cx: Cx,
    DeleteGroupReminder { group_id, id }: DeleteGroupReminder,
) -> Reply<()> {
    let group_id = chat_id(&group_id)?;
    if cx.user.group(group_id).await?.is_none() {
        return Err(ApiError::NotFound {});
    }
    let id: Uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    found(cx.app.store.delete_group_reminder(group_id, id).await?)
}

async fn answer_group_approval(
    cx: Cx,
    AnswerGroupApproval { query_id, allow }: AnswerGroupApproval,
) -> Reply<()> {
    found(
        cx.app
            .answer_group_action(&cx.user, &query_id, allow)
            .await?,
    )
}

async fn leave_group(cx: Cx, LeaveGroup { id }: LeaveGroup) -> Reply<()> {
    let group_id = chat_id(&id)?;
    let members = match cx.user.group(group_id).await? {
        Some(joined) => joined.members,
        None => return Err(ApiError::NotFound {}),
    };
    cx.user.leave_group(group_id).await?;
    for member in &members {
        let _ = cx.app.notifications.send(UserNotice::group_changed(
            &member.user_id,
            &group_id.to_string(),
        ));
    }
    Ok(())
}

/// The text of the user and assistant messages in `entries`, as a
/// conversation a share can keep. Tool results, reasoning, routine prompts
/// and the context around each turn stay private.
fn shared_conversation(entries: &[HistoryEntry]) -> Conversation {
    let at = |millis| DateTime::from_timestamp_millis(millis).unwrap_or_default();
    let mut conversation = Conversation::new();
    for entry in entries {
        let mut shared = match entry {
            HistoryEntry::User { content, .. } => ConversationEntry::user(content.clone()),
            HistoryEntry::Assistant { content, .. } if !content.is_empty() => {
                ConversationEntry::assistant(content.clone())
            }
            HistoryEntry::Assistant { .. }
            | HistoryEntry::Scheduled { .. }
            | HistoryEntry::Approval { .. } => continue,
        };
        if let HistoryEntry::User { timestamp, .. } | HistoryEntry::Assistant { timestamp, .. } =
            entry
        {
            shared.timestamp = at(*timestamp);
        }
        conversation.add(shared);
    }
    conversation
}

/// A share's messages as history: it keeps only user messages and replies.
fn share_history(conversation: &Conversation) -> Vec<HistoryEntry> {
    conversation
        .entries()
        .iter()
        .filter_map(|entry| {
            let timestamp = entry.timestamp.timestamp_millis();
            match &entry.kind {
                EntryKind::User { content, .. } => Some(HistoryEntry::User {
                    content: content.clone(),
                    timestamp,
                    auto: None,
                }),
                EntryKind::Assistant { content, .. } if !content.is_empty() => {
                    Some(HistoryEntry::Assistant {
                        content: content.clone(),
                        timestamp,
                        tools: Vec::new(),
                        memory_score: None,
                        reasoning: String::new(),
                        model: String::new(),
                    })
                }
                _ => None,
            }
        })
        .collect()
}

/// Sessions the database may hand over for a search. Matches only in tool
/// calls or prompts are dropped after, so a search can show fewer.
const SEARCH_CANDIDATES: i64 = 100;
/// Sessions a search returns.
const SEARCH_RESULTS: usize = 30;
async fn search_sessions(
    cx: Cx,
    SearchSessions { query }: SearchSessions,
) -> Reply<Vec<SessionMatch>> {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Ok(Vec::new());
    }
    if query.len() > 200 {
        return Err(ApiError::bad_request("search is at most 200 bytes"));
    }
    let found = cx.user.search_sessions(query, SEARCH_CANDIDATES).await?;
    Ok(found
        .into_iter()
        .filter_map(|found| session_match(found, query))
        .take(SEARCH_RESULTS)
        .collect())
}

/// The latest message the user or Beep wrote in `found` that holds `query`.
fn session_match(found: FoundSession, query: &str) -> Option<SessionMatch> {
    Some(SessionMatch {
        snippet: match_snippet(&found.text, query)?,
        session: found.session,
        from_user: found.from_user,
    })
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
    let connected: Vec<String> = apps
        .iter()
        .filter(|app| app.connected)
        .map(|app| app.slug.clone())
        .collect();
    profile::spawn(&cx.app, &cx.user, &connected);
    Ok(apps)
}

async fn connect_app(cx: Cx, ConnectApp { slug }: ConnectApp) -> Reply<ConnectLink> {
    cx.user.connect_app(&slug).await
}

async fn disconnect_app(cx: Cx, DisconnectApp { slug }: DisconnectApp) -> Reply<()> {
    cx.user.disconnect_app(&slug).await
}

async fn get_models(cx: Cx, _: GetModels) -> Reply<Models> {
    picker(&cx).await
}

/// The caller's picker with their current settings.
async fn picker(cx: &Cx) -> Reply<Models> {
    let model = cx.app.model_for(&cx.user).await;
    let thinking = cx.app.thinking_for(&cx.user).await;
    let auto = cx.user.auto_model().await?;
    let picked = Picked {
        model: &model,
        thinking,
        auto,
    };
    Ok(cx.app.models.for_picker(cx.user.id(), &picked))
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
    picker(&cx).await
}

async fn set_thinking(cx: Cx, SetThinking { level }: SetThinking) -> Reply<Models> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    cx.user.set_thinking(level).await?;
    log::info!("thinking picked; user={} level={level}", cx.user.id());
    picker(&cx).await
}

async fn set_auto_model(cx: Cx, SetAutoModel { enabled }: SetAutoModel) -> Reply<Models> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    cx.user.set_auto_model(enabled).await?;
    log::info!("auto model set; user={} enabled={enabled}", cx.user.id());
    picker(&cx).await
}

async fn get_agents(cx: Cx, _: GetAgents) -> Reply<Agents> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    agents(&cx).await
}

async fn set_agents(cx: Cx, SetAgents { enabled }: SetAgents) -> Reply<Agents> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    if enabled && cx.app.models.agents().is_empty() {
        return Err(ApiError::bad_request("the server has no agents set up"));
    }
    cx.user.set_agents_enabled(enabled).await?;
    log::info!("agents set; user={} enabled={enabled}", cx.user.id());
    agents(&cx).await
}

/// Whether agents are on, with the agents the server has.
async fn agents(cx: &Cx) -> Reply<Agents> {
    let agents = cx
        .app
        .models
        .agents()
        .iter()
        .map(|agent| AgentInfo {
            id: agent.id.as_str().to_owned(),
            name: agent.name.clone(),
            when: agent.when.clone(),
            model: agent.model.to_string(),
            thinking: agent.thinking,
        })
        .collect();
    Ok(Agents {
        enabled: cx.app.store.agents_enabled().await?,
        agents,
    })
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
        agent: None,
        scheduled: None,
        woken: None,
        images: Vec::new(),
        answer: Some(Answer {
            call_id: query_id.clone(),
            decision,
        }),
        model: None,
        thinking: None,
        origin: cx.device,
        platform: None,
        charts: draws_charts(&cx),
    };
    let conversation = cx.user.context(session_id).await?;
    let open = conversation.map(|context| permissions::open_calls(&context.conversation));
    let waiting = open
        .and_then(|open| open.into_iter().next())
        .is_some_and(|call| call.id == query_id);
    let created = if waiting {
        cx.app.submit(&cx.user, message).await?
    } else {
        cx.user
            .existing_run(session_id, &message.request(), &message.message)
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
    found(cx.user.rename_session(chat_id(&id)?, &title).await?)
}

async fn delete_session(cx: Cx, DeleteSession { id }: DeleteSession) -> Reply<()> {
    found(cx.user.delete_session(chat_id(&id)?).await?)
}

async fn delete_sessions(cx: Cx, DeleteSessions { ids }: DeleteSessions) -> Reply<u64> {
    if ids.len() > 200 {
        return Err(ApiError::bad_request("at most 200 sessions at once"));
    }
    let ids: Vec<Uuid> = ids.iter().filter_map(|id| chat_id(id).ok()).collect();
    Ok(cx.user.delete_sessions(&ids).await?)
}

async fn existing_sessions(
    cx: Cx,
    ExistingSessions { ids }: ExistingSessions,
) -> Reply<Vec<String>> {
    if ids.len() > 1000 {
        return Err(ApiError::bad_request("at most 1000 sessions at once"));
    }
    let parsed: Vec<Uuid> = ids.iter().filter_map(|id| chat_id(id).ok()).collect();
    let existing: HashSet<Uuid> = (cx.user.existing_sessions(&parsed).await?)
        .into_iter()
        .collect();
    // Answered in the caller's own spelling of each id.
    Ok(ids
        .into_iter()
        .filter(|id| chat_id(id).is_ok_and(|id| existing.contains(&id)))
        .collect())
}

async fn delete_turn(
    cx: Cx,
    DeleteTurn {
        session_id,
        timestamp,
    }: DeleteTurn,
) -> Reply<()> {
    deleted_turn(
        cx.user
            .delete_turn(chat_id(&session_id)?, timestamp, TurnsToDelete::One)
            .await?,
    )
}

async fn delete_turns_from(
    cx: Cx,
    DeleteTurnsFrom {
        session_id,
        timestamp,
    }: DeleteTurnsFrom,
) -> Reply<()> {
    deleted_turn(
        cx.user
            .delete_turn(chat_id(&session_id)?, timestamp, TurnsToDelete::Onward)
            .await?,
    )
}

fn deleted_turn(deleted: DeletedTurn) -> Reply<()> {
    match deleted {
        DeletedTurn::Deleted => Ok(()),
        DeletedTurn::NotFound => Err(ApiError::NotFound {}),
        DeletedTurn::Busy => Err(ApiError::Conflict {
            message: "the chat has a run in progress".into(),
        }),
    }
}

async fn pin_session(cx: Cx, PinSession { id, pinned }: PinSession) -> Reply<()> {
    found(cx.user.pin_session(chat_id(&id)?, pinned).await?)
}

async fn list_memories(cx: Cx, _: ListMemories) -> Reply<Vec<Memory>> {
    Ok(cx.user.list_memories().await?)
}

async fn list_memory_reads(cx: Cx, _: ListMemoryReads) -> Reply<Vec<MemoryRead>> {
    Ok(profile::memory_reads(&cx.user.memory_rows().await?))
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

fn skill_view(row: SkillRow) -> Skill {
    let time = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Secs, true);
    Skill {
        id: row.id.to_string(),
        name: row.name,
        description: row.description,
        body: row.body,
        group_id: row.group_id.map(|id| id.to_string()),
        group_name: row.group_name,
        enabled: row.enabled,
        uses: u32::try_from(row.uses).unwrap_or(0),
        last_used: row.last_used.map(time),
        updated_at: time(row.updated_at),
        edited: row.edited,
        can_undo: row.can_undo,
    }
}

fn skill_id(id: &str) -> Result<Uuid, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound {})
}

/// `text` trimmed, or a bad request when it is empty or longer than `max`
/// characters.
fn skill_text<'a>(text: &'a str, what: &str, max: usize) -> Result<&'a str, ApiError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max {
        return Err(ApiError::bad_request(format!(
            "a skill's {what} must be 1 to {max} characters"
        )));
    }
    Ok(text)
}

async fn list_skills(cx: Cx, _: ListSkills) -> Reply<Vec<Skill>> {
    Ok(cx
        .user
        .visible_skills()
        .await?
        .into_iter()
        .map(skill_view)
        .collect())
}

async fn update_skill(cx: Cx, req: UpdateSkill) -> Reply<Skill> {
    let id = skill_id(&req.id)?;
    let name = req
        .name
        .as_deref()
        .map(|name| skill_text(name, "name", skills::NAME_MAX))
        .transpose()?;
    let description = req
        .description
        .as_deref()
        .map(|text| skill_text(text, "description", skills::DESCRIPTION_MAX))
        .transpose()?;
    let body = req.body.as_deref().map(str::trim);
    if body.is_some_and(|body| body.is_empty() || body.len() > skills::BODY_MAX) {
        return Err(ApiError::bad_request(format!(
            "a skill's text must be 1 to {} bytes",
            skills::BODY_MAX
        )));
    }
    let change = SkillPatch {
        name,
        description,
        body,
        enabled: req.enabled,
    };
    match cx.user.edit_skill(id, change).await {
        Ok(Some(row)) => Ok(skill_view(row)),
        Ok(None) => Err(ApiError::NotFound {}),
        Err(error) if is_unique_violation(&error) => Err(ApiError::Conflict {
            message: "there is already a skill with that name".into(),
        }),
        Err(error) => Err(error.into()),
    }
}

async fn delete_skill(cx: Cx, DeleteSkill { id }: DeleteSkill) -> Reply<()> {
    found(cx.user.delete_skill(skill_id(&id)?).await?)
}

async fn undo_skill(cx: Cx, UndoSkill { id }: UndoSkill) -> Reply<Skill> {
    match cx.user.undo_skill(skill_id(&id)?).await {
        Ok(Some(row)) => Ok(skill_view(row)),
        Ok(None) => Err(ApiError::NotFound {}),
        Err(error) if is_unique_violation(&error) => Err(ApiError::Conflict {
            message: "another skill has the name this one had".into(),
        }),
        Err(error) => Err(error.into()),
    }
}

/// Whether `error` is the database refusing a second row with the same key.
fn is_unique_violation(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<sqlx::Error>(),
        Some(sqlx::Error::Database(error)) if error.is_unique_violation()
    )
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
        .create_cron(&req.label, &req.schedule, &req.prompt)
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
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
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
    let device = PushDevice {
        endpoint: cx.connection.remote_id(),
        device_id: &req.device_id,
        fcm_token: &req.fcm_token,
    };
    cx.user.register_device(device).await?;
    Ok(())
}

async fn unregister_device(cx: Cx, req: UnregisterDevice) -> Reply<()> {
    found(cx.user.unregister_device(&req.device_id).await?)
}

/// Links a computer whose offer the phone scanned. Only the primary key
/// may link others.
async fn link_device(cx: Cx, LinkDevice { offer }: LinkDevice) -> Reply<LinkedDevice> {
    if !cx.primary {
        return Err(ApiError::Unauthorized {});
    }
    offer
        .verify(cx.pusher.server_id(), SystemTime::now())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let key = NewKey {
        endpoint: offer.device,
        name: offer.name.trim(),
        kind: DeviceKind::Desktop {},
    };
    match cx.user.link_key(key).await? {
        LinkKey::Linked(key) => {
            // Also the new device's first notice, which tells it it is linked.
            cx.user.changed("");
            Ok(linked_device(&cx, key))
        }
        LinkKey::Taken => Err(ApiError::Conflict {
            message: "that device is already linked to an account".into(),
        }),
    }
}

async fn list_devices(cx: Cx, _: ListDevices) -> Reply<Vec<LinkedDevice>> {
    let keys = cx.user.account_keys().await?;
    Ok(keys
        .into_iter()
        .map(|key| linked_device(&cx, key))
        .collect())
}

/// The primary key removes any linked one; a linked key only itself.
async fn remove_device(cx: Cx, RemoveDevice { id }: RemoveDevice) -> Reply<()> {
    let caller = cx.connection.remote_id();
    if !cx.primary && id != caller.to_string() {
        return Err(ApiError::Unauthorized {});
    }
    let key: EndpointId = id
        .parse()
        .map_err(|_| ApiError::bad_request("not a device id"))?;
    found(cx.user.revoke_key(&id).await?)?;
    cx.user.changed("");
    cx.pusher.forget(key);
    Ok(())
}

/// A code another device joins the caller's account with. Only the
/// primary key may invite.
async fn invite_device(cx: Cx, InviteDevice { name }: InviteDevice) -> Reply<DeviceInvite> {
    if !cx.primary {
        return Err(ApiError::Unauthorized {});
    }
    let name = name.trim();
    if name.is_empty() || name.len() > link::MAX_NAME {
        return Err(ApiError::bad_request(
            "the device name is empty or too long",
        ));
    }
    let invite = cx
        .user
        .create_invite(&cx.connection.remote_id(), name, INVITE_TTL)
        .await?;
    Ok(DeviceInvite {
        uri: link::join_uri(&invite.code),
        code: invite.code,
        expires_at: invite.expires_at.into(),
    })
}

async fn peek_invite(cx: Cx, PeekInvite { code }: PeekInvite) -> Reply<JoinPreview> {
    let inviter = inviter(&cx, &code).await?;
    Ok(JoinPreview {
        name: inviter.name,
        kind: inviter.kind,
    })
}

/// The account behind a live code other than the caller's own.
async fn inviter(cx: &Cx, code: &str) -> Result<Inviter, ApiError> {
    let code = link::join_code(code).ok_or_else(|| ApiError::bad_request("not a join code"))?;
    let inviter = cx
        .app
        .store
        .peek_invite(&code)
        .await?
        .ok_or(ApiError::NotFound {})?;
    if inviter.account.id == cx.user.id() {
        return Err(ApiError::Conflict {
            message: "that code is for this account".into(),
        });
    }
    Ok(inviter)
}

/// Moves the caller's account into the one that made `code`, then closes
/// the connections of its keys so they sign in as the account joined.
async fn join_account(cx: Cx, JoinAccount { code }: JoinAccount) -> Reply<()> {
    if !cx.primary {
        return Err(ApiError::Unauthorized {});
    }
    let inviter = inviter(&cx, &code).await?;
    let target = cx.app.user_for(&inviter.account);
    // The apps connected under the joining account stay connected when the
    // account joined has none of its own.
    let (mine, theirs) = tokio::join!(cx.user.composio_connected(), target.composio_connected());
    let into = MergeInto {
        code: &link::join_code(&code).unwrap_or_default(),
        target: target.id(),
        adopt_external_id: mine == Some(true) && theirs == Some(false),
    };
    match cx.user.merge_into(into).await? {
        Merge::Merged { target, keys } => {
            log::info!("account merged; from={} into={target}", cx.user.id());
            cx.app.merged(cx.user.id(), &target).await?;
            if let Some(user) = cx.app.user(&target) {
                user.changed("");
            }
            let pusher = cx.pusher.clone();
            tokio::spawn(async move {
                sleep(REJOIN_AFTER).await;
                for key in keys.iter().filter_map(|key| key.parse().ok()) {
                    pusher.reconnect(key);
                }
            });
            Ok(())
        }
        Merge::NoInvite => Err(ApiError::NotFound {}),
        Merge::SameAccount => Err(ApiError::Conflict {
            message: "that code is for this account".into(),
        }),
        Merge::Busy => Err(ApiError::Conflict {
            message: "a chat is still running; wait for it or stop it, then join".into(),
        }),
    }
}

fn linked_device(cx: &Cx, key: AccountKey) -> LinkedDevice {
    LinkedDevice {
        this_device: key.endpoint_id == cx.connection.remote_id().to_string(),
        id: key.endpoint_id,
        name: key.name,
        kind: key.kind,
        primary: key.primary,
        linked_at: key.linked_at.into(),
    }
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
    let before = cx.user.get_location().await?;
    cx.user.update_location(&fix).await?;
    // A failure here must not lose the fix, which is already saved.
    if let Err(error) = places::track(&cx.app, &cx.user, &fix).await {
        log::warn!(
            "places: unable to check the fix: {error:#}; user={}",
            cx.user.id()
        );
    }
    if let Some(event) = before.and_then(|before| wake::arrival(&before, &fix)) {
        log::info!("wake: arrival; user={}", cx.user.id());
        let (app, user) = (cx.app.clone(), cx.user.clone());
        tokio::spawn(async move {
            if let Err(error) = wake::wake(&app, &user, event).await {
                log::warn!("wake: unable to start the run: {error:#}; user={user}");
            }
        });
    }
    Ok(())
}

fn place_info(place: SavedPlace) -> Place {
    let time = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Secs, true);
    Place {
        id: place.id.to_string(),
        name: place.name,
        latitude: place.latitude,
        longitude: place.longitude,
        radius: place.radius,
        address: place.address,
        created_at: time(place.created_at),
        here: place.inside,
        last_here_at: place.last_here_at.map(time),
        routines: place.routines,
    }
}

async fn list_places(cx: Cx, _: ListPlaces) -> Reply<Vec<Place>> {
    let places = cx.user.list_places().await?;
    Ok(places.into_iter().map(place_info).collect())
}

async fn delete_place(cx: Cx, DeletePlace { id }: DeletePlace) -> Reply<()> {
    let id: Uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    let deleted = cx.user.delete_place(id).await?;
    if deleted {
        // Its routines went with it.
        cx.user.changed("");
    }
    found(deleted)
}

async fn update_place(cx: Cx, req: UpdatePlace) -> Reply<Place> {
    let id: Uuid = req.id.parse().map_err(|_| ApiError::NotFound {})?;
    let place = cx
        .user
        .update_place(id, req.name.as_deref(), req.radius)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    place.map(place_info).ok_or(ApiError::NotFound {})
}

async fn cross_place(cx: Cx, CrossPlace { id, crossing }: CrossPlace) -> Reply<bool> {
    let id: Uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    let changed = places::cross(&cx.app, &cx.user, id, crossing.into()).await?;
    if changed {
        cx.user.changed("");
    }
    Ok(changed)
}

async fn set_timezone(cx: Cx, SetTimezone { timezone }: SetTimezone) -> Reply<()> {
    let timezone: Tz = timezone
        .parse()
        .map_err(|_| ApiError::bad_request("unknown timezone"))?;
    cx.user.set_timezone(timezone.name()).await?;
    Ok(())
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

/// The caller's updates after `after`, a page at a time.
async fn get_updates(cx: Cx, GetUpdates { after }: GetUpdates) -> Reply<UpdatePage> {
    Ok(cx.user.updates_page(after).await?)
}

/// Sends the caller's newest update id now and each time it moves, until
/// the connection closes. The client then asks `GetUpdates` for what is new.
async fn stream_updates(cx: Cx, _: StreamUpdates, sink: Sink<i64>) -> Reply<()> {
    let mut last = cx.user.watch_updates().await?;
    loop {
        let newest = *last.borrow_and_update();
        match timeout(SEND_TIMEOUT, sink.send(&newest)).await {
            Ok(sent) => sent?,
            Err(_) => return Ok(()),
        }
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            changed = last.changed() => if changed.is_err() {
                return Ok(());
            },
        }
    }
}

/// A chat id from a request. One that is not a UUID names no chat.
fn chat_id(id: &str) -> Reply<Uuid> {
    Uuid::parse_str(id).map_err(|_| ApiError::NotFound {})
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
    next_run(&decode_schedule(schedule.to_owned()), tz_name)
}

/// When `schedule` next fires, reading a cron expression in `tz_name`.
fn next_run(schedule: &JobSchedule, tz_name: &str) -> Option<DateTime<Utc>> {
    let expression = match schedule {
        JobSchedule::Once(at) => return Some(*at),
        JobSchedule::Cron(expression) => expression,
        JobSchedule::OnWake { .. } => return None,
    };
    let tz: Tz = tz_name.parse().unwrap_or(UTC);
    CronSchedule::parse(expression)
        .ok()?
        .next_after_in_tz(&Utc::now(), tz)
        .map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::types::ToolCall;
    use serde_json::json;

    fn refusal<T>(reply: Reply<T>) -> String {
        match reply {
            Err(ApiError::BadRequest { message }) => message,
            _ => panic!("expected a bad request"),
        }
    }

    #[test]
    fn only_members_may_not_go_by_beep() {
        assert_eq!(refusal(group_member_name(" beep ")), "Beep is taken");
        assert_eq!(group_member_name(" Ana ").ok().as_deref(), Some("Ana"));
        assert_eq!(group_name("Beep").ok().as_deref(), Some("Beep"));
        assert_eq!(
            refusal(group_member_name(&"a".repeat(31))),
            "member name must be 1-30 characters"
        );
        assert_eq!(
            refusal(group_name(" ")),
            "group name must be 1-60 characters"
        );
    }

    #[test]
    fn too_many_or_too_large_images_are_refused() {
        let image = || "x".to_owned();
        assert_eq!(
            refusal(check_images(&vec![image(); MAX_IMAGES + 1])),
            "at most 8 images per message"
        );
        assert_eq!(
            refusal(check_images(&["x".repeat(MAX_IMAGE_BYTES + 1)])),
            "each image must be at most 2 MiB"
        );
        assert!(check_images(&vec![image(); MAX_IMAGES]).is_ok());
    }

    #[test]
    fn every_op_is_routed_once() {
        super::router().check().unwrap();
    }

    #[test]
    fn a_share_keeps_only_the_message_text() {
        let mut chat = Conversation::new();
        chat.add(ConversationEntry::system_message("Location: home"));
        chat.add(ConversationEntry::user("check my mail"));
        chat.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "mail".into(),
                arguments: json!({}),
            }],
            Some("thinking".into()),
        ));
        chat.add(ConversationEntry::tool("c1", "secret inbox"));
        chat.add(ConversationEntry::assistant_with_reasoning(
            "Two new emails.",
            Some("more thinking".into()),
        ));
        let shared = shared_conversation(&share_history(&chat));
        let json = serde_json::to_string(&shared).unwrap();
        for private in ["home", "secret", "thinking", "c1"] {
            assert!(!json.contains(private), "{private} leaked into {json}");
        }
        let texts: Vec<_> = shared.entries().iter().map(|e| e.content()).collect();
        assert_eq!(texts, ["check my mail", "Two new emails."]);
    }

    #[test]
    fn search_matches_messages_ignoring_case() {
        let found = |text: &str, from_user| FoundSession {
            session: Session {
                id: "s".into(),
                title: "Emails".into(),
                pinned: false,
                updated_at: 0,
            },
            text: text.to_owned(),
            from_user,
        };
        let reply = || found("The **Kimpton** offers a free transfer.", false);
        let hit = session_match(reply(), "kimpton").unwrap();
        assert_eq!(hit.snippet, "The Kimpton offers a free transfer.");
        assert!(!hit.from_user);
        assert!(
            session_match(found("Any news from the hotel?", true), "HOTEL")
                .unwrap()
                .from_user
        );
        assert!(session_match(reply(), "boiler").is_none());
    }
}
