//! Beep wire protocol over iroh.
//!
//! Frames are length-delimited msgpack holding the types in `protocol`.
//!
//! The first bi stream on a connection carries `Request::Auth`; the server
//! answers `Reply::Done` or closes the connection. After that each request gets
//! its own bi stream: one request frame in, reply frames out until the server
//! finishes the stream.

use crate::{
    app::{App, Message},
    protocol::{HistoryEntry, Reply, Request, RunEvent},
    store::CreateRun,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use flashmind::{core::EntryKind, cron::CronSchedule};
use futures::{SinkExt, StreamExt};
use iroh::{
    endpoint::{Connection, RecvStream, SendStream, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt, sync::Arc, time::Duration};
use tokio::{
    sync::{Semaphore, broadcast::error::RecvError},
    time::{Instant, sleep, timeout},
};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

pub const ALPN: &[u8] = b"beep/1";

const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_STREAMS: usize = 32;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_UNAUTHORIZED: u32 = 401;

/// Error reply from the server, recoverable from `anyhow::Error` by downcast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteError {
    pub code: u16,
    pub message: String,
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "server error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RemoteError {}

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME)
        .new_codec()
}

pub struct Tx(FramedWrite<SendStream, LengthDelimitedCodec>);
pub struct Rx(FramedRead<RecvStream, LengthDelimitedCodec>);

pub fn framed(send: SendStream, recv: RecvStream) -> (Tx, Rx) {
    (
        Tx(FramedWrite::new(send, codec())),
        Rx(FramedRead::new(recv, codec())),
    )
}

impl Tx {
    pub async fn send<T: Serialize>(&mut self, msg: &T) -> Result<()> {
        self.0.send(rmp_serde::to_vec(msg)?.into()).await?;
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        self.0.get_mut().finish()?;
        Ok(())
    }

    async fn reply(&mut self, reply: Reply) -> Result<()> {
        self.send(&reply).await
    }

    async fn done(&mut self) -> Result<()> {
        self.send(&Reply::Done {}).await
    }

    async fn err(&mut self, code: u16, message: &str) -> Result<()> {
        let message = message.to_owned();
        self.send(&Reply::Error { code, message }).await
    }
}

impl Rx {
    /// Next message, `None` once the peer finished the stream.
    pub async fn next<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        match self.0.next().await {
            Some(frame) => Ok(Some(rmp_serde::from_slice(&frame?)?)),
            None => Ok(None),
        }
    }
}

#[derive(Clone)]
pub struct BeepHandler {
    app: Arc<App>,
}

impl fmt::Debug for BeepHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BeepHandler").finish()
    }
}

impl BeepHandler {
    pub fn new(app: Arc<App>) -> Self {
        Self { app }
    }
}

impl ProtocolHandler for BeepHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let user = match authenticate(&self.app, &connection).await {
            Ok(user) => user,
            Err(e) => {
                log::debug!("connection rejected: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAUTHORIZED), b"unauthorized");
                return Ok(());
            }
        };
        let user: Arc<str> = user.into();
        let streams = Arc::new(Semaphore::new(MAX_STREAMS));
        while let Ok((send, recv)) = connection.accept_bi().await {
            let Ok(permit) = streams.clone().acquire_owned().await else {
                break;
            };
            let app = self.app.clone();
            let user = user.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(e) = serve_stream(app, &user, send, recv).await {
                    log::debug!("stream ended: {e:#}");
                }
            });
        }
        Ok(())
    }
}

async fn authenticate(app: &App, connection: &Connection) -> Result<String> {
    let (send, recv) = timeout(READ_TIMEOUT, connection.accept_bi())
        .await
        .context("auth timed out")??;
    let (mut tx, mut rx) = framed(send, recv);
    let request = timeout(READ_TIMEOUT, rx.next())
        .await
        .context("auth timed out")??
        .context("auth stream empty")?;
    let Request::Auth { token } = request else {
        bail!("first request must be Auth");
    };
    let user = app
        .auth
        .authenticate(Some(&format!("Bearer {token}")))
        .ok_or_else(|| anyhow!("invalid token"))?;
    tx.done().await?;
    tx.finish()?;
    Ok(user)
}

async fn serve_stream(app: Arc<App>, user: &str, send: SendStream, recv: RecvStream) -> Result<()> {
    let (mut tx, mut rx) = framed(send, recv);
    let request = timeout(READ_TIMEOUT, rx.next::<Request>())
        .await
        .context("request timed out")?;
    let result = match request.and_then(|r| r.context("empty stream")) {
        Ok(request) => handle(&app, user, request, &mut tx).await,
        Err(e) => tx.err(400, &format!("{e:#}")).await,
    };
    tx.finish()?;
    result
}

async fn handle(app: &Arc<App>, user: &str, request: Request, tx: &mut Tx) -> Result<()> {
    match request {
        Request::Auth { .. } => tx.err(400, "already authenticated").await,
        Request::Health {} => tx.done().await,
        Request::Chat {
            session_id,
            request_id,
            message,
            image,
        } => {
            if message.trim().is_empty() || message.len() > 32768 {
                return tx.err(400, "message must be 1-32768 bytes").await;
            }
            let m = Message {
                session_id,
                request_id,
                message,
                cron_id: None,
                image,
            };
            match app.submit(user, m).await? {
                CreateRun::Created(run) | CreateRun::Existing(run) => {
                    tx.reply(Reply::Run { run }).await
                }
                CreateRun::Conflict => tx.err(409, "session busy").await,
            }
        }
        Request::GetRun { id } => match app.store.run(user, &id).await? {
            Some(run) => tx.reply(Reply::Run { run }).await,
            None => not_found(tx).await,
        },
        Request::CancelRun { id } => {
            if app.store.run(user, &id).await?.is_none() {
                return not_found(tx).await;
            }
            app.cancel(&id).await;
            tx.done().await
        }
        Request::GetEvents { run_id, after } => {
            if after < 0 {
                return tx.err(400, "after must be >= 0").await;
            }
            if app.store.run(user, &run_id).await?.is_none() {
                return not_found(tx).await;
            }
            stream_events(app, user, &run_id, after, tx).await
        }
        Request::DeviceResult {
            run_id,
            query_id,
            answer,
        } => {
            if app.store.run(user, &run_id).await?.is_none() {
                return not_found(tx).await;
            }
            let resolved = app.resolve_device_query(&query_id, answer).await;
            tx.reply(Reply::Resolved { resolved }).await
        }
        Request::ListSessions {} => {
            let sessions = app.store.sessions(user).await?;
            tx.reply(Reply::Sessions { sessions }).await
        }
        Request::GetSession { id } => {
            let Some(conv) = app.store.conversation(user, &id).await? else {
                return not_found(tx).await;
            };
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
            tx.reply(Reply::History { entries }).await
        }
        Request::RenameSession { id, title } => {
            if title.is_empty() {
                return tx.err(400, "title required").await;
            }
            found(tx, app.store.rename_session(user, &id, &title).await?).await
        }
        Request::DeleteSession { id } => {
            found(tx, app.store.delete_session(user, &id).await?).await
        }
        Request::PinSession { id, pinned } => {
            found(tx, app.store.pin_session(user, &id, pinned).await?).await
        }
        Request::ListMemories {} => {
            let memories = app.store.list_memories(user).await?;
            tx.reply(Reply::Memories { memories }).await
        }
        Request::DeleteMemory { key } => found(tx, app.store.forget(user, &key).await?).await,
        Request::ListCrons {} => {
            let tz = app
                .store
                .get_timezone(user)
                .await
                .unwrap_or_else(|_| "UTC".into());
            let mut crons = app.store.list_crons(user).await?;
            for c in &mut crons {
                c.next_run_at = next_run_at(&c.schedule, &tz);
            }
            tx.reply(Reply::Crons { crons }).await
        }
        Request::CreateCron {
            label,
            prompt,
            schedule,
        } => {
            match app
                .store
                .create_cron(user, &label, &prompt, &schedule)
                .await
            {
                Ok(id) => tx.reply(Reply::Created { id }).await,
                Err(e) => tx.err(400, &e.to_string()).await,
            }
        }
        Request::ToggleCron { id } => match app.store.toggle_cron(user, &id).await? {
            Some(enabled) => tx.reply(Reply::Enabled { enabled }).await,
            None => not_found(tx).await,
        },
        Request::DeleteCron { id } => found(tx, app.store.delete_cron(user, &id).await?).await,
        Request::UpdateCron {
            id,
            enabled,
            label,
            schedule,
        } => {
            let updated = app
                .store
                .update_cron(user, &id, enabled, label.as_deref(), schedule.as_deref())
                .await?;
            found(tx, updated).await
        }
        Request::RegisterDevice {
            device_id,
            fcm_token,
        } => {
            app.store
                .register_device(user, &device_id, &fcm_token)
                .await?;
            tx.done().await
        }
        Request::UnregisterDevice { device_id } => {
            found(tx, app.store.unregister_device(user, &device_id).await?).await
        }
        Request::UpdateLocation {
            latitude,
            longitude,
            accuracy,
            timezone,
        } => {
            let valid = latitude.is_finite()
                && longitude.is_finite()
                && accuracy.is_finite()
                && (-90.0..=90.0).contains(&latitude)
                && (-180.0..=180.0).contains(&longitude);
            if !valid {
                return tx.err(400, "invalid coordinates").await;
            }
            app.store
                .update_location_tz(user, latitude, longitude, accuracy, &timezone)
                .await?;
            tx.done().await
        }
        Request::SaveNotifications { notifications } => {
            let count = app.store.save_notifications(user, notifications).await?;
            tx.reply(Reply::Saved {
                count: count as u64,
            })
            .await
        }
        Request::StreamNotifications {} => stream_notifications(app, user, tx).await,
        Request::ListIntegrations {} => {
            let integrations = app.store.list_mcp_connections(user).await?;
            tx.reply(Reply::Integrations { integrations }).await
        }
        Request::CreateIntegration {
            provider,
            label,
            config,
        } => {
            match app
                .store
                .create_mcp_connection(user, &provider, &label, &config)
                .await
            {
                Ok(id) => tx.reply(Reply::Created { id }).await,
                Err(e) => tx.err(400, &e.to_string()).await,
            }
        }
        Request::DeleteIntegration { id } => {
            found(tx, app.store.delete_mcp_connection(user, &id).await?).await
        }
        Request::UpdateCredentials {
            id,
            credentials,
            status,
        } => {
            found(
                tx,
                app.store
                    .update_mcp_credentials(user, &id, &credentials, &status)
                    .await?,
            )
            .await
        }
    }
}

async fn not_found(tx: &mut Tx) -> Result<()> {
    tx.err(404, "not found").await
}

async fn found(tx: &mut Tx, found: bool) -> Result<()> {
    if found {
        tx.done().await
    } else {
        not_found(tx).await
    }
}

fn next_run_at(expression: &str, tz_name: &str) -> Option<String> {
    let schedule = CronSchedule::parse(expression).ok()?;
    let tz: chrono_tz::Tz = tz_name.parse().unwrap_or(chrono_tz::UTC);
    schedule
        .next_after_in_tz(&Utc::now(), tz)
        .map(|t| t.to_rfc3339())
}

async fn stream_events(
    app: &App,
    user: &str,
    run_id: &str,
    mut after: i64,
    tx: &mut Tx,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let running = app
            .store
            .run(user, run_id)
            .await?
            .is_some_and(|r| r.status == "running");
        let events = app.store.events(run_id, after).await?;
        let empty = events.is_empty();
        for event in events {
            after = event.seq;
            let finished = matches!(event.event, RunEvent::RunFinished { .. });
            tx.reply(Reply::Event {
                seq: event.seq,
                event: event.event,
            })
            .await?;
            if finished {
                return Ok(());
            }
        }
        if (empty && !running) || Instant::now() >= deadline {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn stream_notifications(app: &App, user: &str, tx: &mut Tx) -> Result<()> {
    let mut rx = app.notifications.subscribe();
    loop {
        tokio::select! {
            _ = sleep(Duration::from_secs(30)) => {
                if timeout(READ_TIMEOUT, tx.send(&Reply::Heartbeat {})).await.is_err() {
                    return Ok(());
                }
            }
            result = rx.recv() => match result {
                Ok(event) => {
                    if event.user != user {
                        continue;
                    }
                    let reply = Reply::Notice { notice: event.notice };
                    if timeout(READ_TIMEOUT, tx.reply(reply)).await.is_err() {
                        return Ok(());
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }
}
