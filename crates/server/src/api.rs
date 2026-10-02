//! Request handlers, one function per request type.

use crate::{
    app::Message,
    protocol::*,
    router::{Cx, Router, Sink},
    store::CreateRun,
};
use chrono::Utc;
use flashmind::{core::EntryKind, cron::CronSchedule};
use std::time::Duration;
use tokio::{
    sync::broadcast::error::RecvError,
    time::{Instant, sleep, timeout},
};

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const EVENTS_WINDOW: Duration = Duration::from_secs(30);

pub fn router() -> Router {
    Router::default()
        .call(auth)
        .call(health)
        .call(chat)
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
}

type Reply<T> = Result<T, ApiError>;

/// Auth is handled when the connection opens; a second one is a mistake.
async fn auth(_: Cx, _: Auth) -> Reply<()> {
    Err(ApiError::bad_request("already authenticated"))
}

async fn health(_: Cx, _: Health) -> Reply<()> {
    Ok(())
}

async fn chat(cx: Cx, req: Chat) -> Reply<Run> {
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
        CreateRun::Created(run) | CreateRun::Existing(run) => Ok(run),
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

/// Sends the run's events after `after`, following it while it runs, for up
/// to `EVENTS_WINDOW`. Clients reconnect with the last `seq` they saw.
async fn get_events(cx: Cx, req: GetEvents, sink: Sink<Event>) -> Reply<()> {
    let GetEvents { run_id, mut after } = req;
    if after < 0 {
        return Err(ApiError::bad_request("after must be >= 0"));
    }
    owned_run(&cx, &run_id).await?;
    let deadline = Instant::now() + EVENTS_WINDOW;
    loop {
        let running = cx
            .app
            .store
            .run(&cx.user, &run_id)
            .await?
            .is_some_and(|r| r.status == "running");
        let events = cx.app.store.events(&run_id, after).await?;
        let empty = events.is_empty();
        for event in events {
            after = event.seq;
            sink.send(&event).await?;
            if matches!(event.event, RunEvent::RunFinished { .. }) {
                return Ok(());
            }
        }
        if (empty && !running) || Instant::now() >= deadline {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
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
    let entries = conv
        .entries()
        .iter()
        .filter_map(|e| match &e.kind {
            EntryKind::User { content, .. } => Some(HistoryEntry::User {
                content: content.clone(),
                timestamp: e.timestamp,
            }),
            EntryKind::Assistant { content, .. } => Some(HistoryEntry::Assistant {
                content: content.clone(),
                timestamp: e.timestamp,
            }),
            _ => None,
        })
        .collect();
    Ok(entries)
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
    #[test]
    fn every_op_is_routed_once() {
        super::router().check().unwrap();
    }
}
