//! Request handlers, one function per request type.

use crate::{
    app::{Answer, Draws, Message},
    images::{MAX_IMAGE_BYTES, MAX_IMAGES},
    live::Subscription,
    permissions,
    protocol::*,
    router::{Cx, Router, Sink},
    store::{CreateRun, decode_schedule},
    tools::stocks::{self, StockError, Symbol},
};
use chrono::{DateTime, Utc};
use chrono_tz::{Tz, UTC};
use flashmind::cron::{CronSchedule, JobSchedule};
use std::{collections::VecDeque, time::Duration};
use tokio::{sync::broadcast::error::RecvError, time::timeout};
use uuid::Uuid;

mod crons;
mod devices;
mod groups;
mod location;
mod memories;
mod sessions;
mod settings;

use crons::{create_cron, delete_cron, list_crons, run_cron, toggle_cron, update_cron};
use devices::{
    invite_device, join_account, link_device, list_devices, peek_invite, register_device,
    remove_device, unregister_device,
};
use groups::{
    answer_group_approval, create_group, delete_group_reminder, get_group, get_group_images,
    group_invite, join_group, leave_group, list_group_reminders, list_groups, peek_group,
    pin_group, send_to_group, share_group_apps,
};
use location::{
    cross_place, delete_place, list_places, set_timezone, update_location, update_place,
};
use memories::{
    clear_memories, delete_memory, delete_skill, list_memories, list_memory_reads, list_skills,
    undo_skill, update_memory, update_skill,
};
use sessions::{
    copy_share, delete_session, delete_sessions, delete_turn, delete_turns_from, existing_sessions,
    get_file, get_route, get_route_card, get_session, get_session_agents, get_session_images,
    get_share, list_sessions, pin_session, rename_session, search_sessions, share_session,
    unshare_session,
};
use settings::{
    add_app_account, add_mcp_server, connect_app, connect_mcp_server, disconnect_app,
    finish_app_auth, finish_mcp_auth, get_agents, get_app_permissions, get_models,
    list_app_accounts, list_apps, list_mcp_servers, remove_app_account, remove_mcp_server,
    set_agents, set_app_permissions, set_auto_model, set_model, set_thinking, start_app_auth,
};

const SEND_TIMEOUT: Duration = Duration::from_secs(10);

pub fn router() -> Router {
    Router::default()
        .call(hello)
        .call(health)
        .stream(chat)
        .call(get_run)
        .call(active_run)
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
        .call(list_app_accounts)
        .call(add_app_account)
        .call(remove_app_account)
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
        .call(get_route)
        .call(get_route_card)
        .call(list_mcp_servers)
        .call(add_mcp_server)
        .call(connect_mcp_server)
        .call(finish_mcp_auth)
        .call(remove_mcp_server)
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
        always_sent: false,
        woken: None,
        images: req.images,
        answer: None,
        model,
        thinking: req.thinking,
        origin: cx.device,
        platform: req.platform,
        draws: draws(&cx),
    };
    match cx.app.submit(&cx.user, message).await? {
        CreateRun::Created(run) | CreateRun::Existing(run) => follow(&cx, run, 0, &sink).await,
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}

/// What a run started on this connection can show. Only the macOS app draws
/// charts, and the iOS app does not draw cards yet.
fn draws(cx: &Cx) -> Draws {
    Draws {
        charts: cx.device == (DeviceKind::Desktop {}) && cx.supports(Feature::CHARTS),
        cards: cx.supports(Feature::CARDS) && cx.client.platform != Some(Platform::Ios {}),
    }
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

async fn active_run(cx: Cx, ActiveRun { session_id }: ActiveRun) -> Reply<Option<Run>> {
    Ok(cx.user.active_run(session_id).await?)
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

/// Sends the run's events after `after` and follows it until it finishes. A
/// finished run keeps no events, only its status: the turn itself is in the
/// session history. A client that drops the stream asks again with the last
/// `seq` it saw.
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
}

async fn device_result(cx: Cx, req: DeviceResult) -> Reply<bool> {
    owned_run(&cx, &req.run_id).await?;
    Ok(cx.app.resolve_device_query(&req.query_id, req.answer).await)
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
        always_sent: false,
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
        draws: draws(&cx),
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

async fn suggest_prompts(
    cx: Cx,
    SuggestPrompts { refresh }: SuggestPrompts,
) -> Reply<PromptSuggestions> {
    Ok(cx.app.suggest_prompts(&cx.user, refresh).await)
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

    pub(super) fn refusal<T>(reply: Reply<T>) -> String {
        match reply {
            Err(ApiError::BadRequest { message }) => message,
            _ => panic!("expected a bad request"),
        }
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
}
