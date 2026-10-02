use crate::{
    app::{App, Message},
    store::CreateRun,
};
use anyhow::Result;
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use flashmind::cron::CronSchedule;
use http::{Request, Response, StatusCode};
use iroh::endpoint::Connection;
use iroh::protocol::AcceptError;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub const ALPN: &[u8] = b"beep/1";

pub(crate) struct Stream {
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
}

impl Stream {
    async fn recv_data(&mut self) -> Result<Option<Bytes>> {
        let mut buf = vec![0u8; 8192];
        match self.recv.read(&mut buf).await? {
            Some(n) => {
                buf.truncate(n);
                Ok(Some(Bytes::from(buf)))
            }
            None => Ok(None),
        }
    }
    async fn send_response(&mut self, response: Response<()>) -> Result<()> {
        let status = response.status().as_u16();
        self.send.write_all(&status.to_be_bytes()).await?;
        Ok(())
    }
    async fn send_data(&mut self, bytes: Bytes) -> Result<()> {
        self.send.write_all(&bytes).await?;
        Ok(())
    }
    async fn finish(&mut self) -> Result<()> {
        self.send.finish().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct BeepHandler {
    app: Arc<App>,
}

impl std::fmt::Debug for BeepHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BeepHandler").finish()
    }
}

impl BeepHandler {
    pub fn new(app: Arc<App>) -> Self {
        Self { app }
    }
}

impl iroh::protocol::ProtocolHandler for BeepHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let connections = Arc::new(tokio::sync::Semaphore::new(32));
        loop {
            let (send, recv) = match connection.accept_bi().await {
                Ok(streams) => streams,
                Err(_) => break,
            };
            let Ok(permit) = connections.clone().try_acquire_owned() else {
                continue;
            };
            let app = self.app.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(error) = handle_iroh_stream(app, send, recv).await {
                    log::debug!("iroh request ended: {error}");
                }
            });
        }
        Ok(())
    }
}

async fn handle_iroh_stream(
    app: Arc<App>,
    send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> Result<()> {
    let mut len_buf = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(10), recv.read_exact(&mut len_buf))
        .await
        .map_err(|_| anyhow::anyhow!("request header read timed out"))??;
    let header_len = u32::from_be_bytes(len_buf) as usize;
    anyhow::ensure!(header_len <= 8192, "request header too large");
    let mut header_buf = vec![0u8; header_len];
    tokio::time::timeout(Duration::from_secs(10), recv.read_exact(&mut header_buf))
        .await
        .map_err(|_| anyhow::anyhow!("request header read timed out"))??;
    let header: Value = serde_json::from_slice(&header_buf)?;
    let method = header["m"].as_str().unwrap_or("GET");
    let path = header["p"].as_str().unwrap_or("/");
    let auth = header["a"].as_str().unwrap_or("");
    let mut builder = Request::builder().method(method).uri(path);
    if !auth.is_empty() {
        builder = builder.header("authorization", auth);
    }
    let request = builder.body(())?;
    let stream = Stream { send, recv };
    handle(app, request, stream).await
}

async fn read_body(stream: &mut Stream, limit: usize) -> Result<Option<Bytes>> {
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut body = BytesMut::new();
        while let Some(chunk) = stream.recv_data().await? {
            if body.len() + chunk.len() > limit {
                return Ok::<_, anyhow::Error>(None);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Some(body.freeze()))
    })
    .await;
    match result {
        Err(_) => Ok(None),
        Ok(inner) => inner,
    }
}

async fn reply(stream: &mut Stream, status: StatusCode, value: Value) -> Result<()> {
    stream
        .send_response(
            Response::builder()
                .status(status)
                .header("content-type", "application/json")
                .header("cache-control", "no-store")
                .body(())?,
        )
        .await?;
    stream
        .send_data(Bytes::from(serde_json::to_vec(&value)?))
        .await?;
    stream.finish().await?;
    Ok(())
}

fn next_run_at(expression: &str, tz_name: &str) -> Option<String> {
    let schedule = CronSchedule::parse(expression).ok()?;
    let tz: chrono_tz::Tz = tz_name.parse().unwrap_or(chrono_tz::UTC);
    let next = schedule.next_after_in_tz(&Utc::now(), tz)?;
    Some(next.to_rfc3339())
}

/// Read the request body, parse it as JSON, and reply with an error if either step fails.
/// Returns `None` (after sending the error reply) when the body is too large or not valid JSON.
async fn read_json(stream: &mut Stream, limit: usize) -> Result<Option<Value>> {
    let Some(body) = read_body(stream, limit).await? else {
        reply(
            stream,
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({"error":"body too large"}),
        )
        .await?;
        return Ok(None);
    };
    match serde_json::from_slice(&body) {
        Ok(v) => Ok(Some(v)),
        Err(_) => {
            reply(
                stream,
                StatusCode::BAD_REQUEST,
                json!({"error":"invalid JSON"}),
            )
            .await?;
            Ok(None)
        }
    }
}

fn ok_or_not_found(found: bool, key: &str) -> (StatusCode, Value) {
    if found {
        (StatusCode::OK, json!({key: true}))
    } else {
        (StatusCode::NOT_FOUND, json!({key: false}))
    }
}

pub(crate) async fn handle(app: Arc<App>, request: Request<()>, mut stream: Stream) -> Result<()> {
    let path = request.uri().path();
    let method = request.method().as_str();

    if method == "GET" && path == "/v1/auth" {
        let mode = if app.auth.is_testing() {
            "testing"
        } else {
            "token"
        };
        return reply(&mut stream, StatusCode::OK, json!({"mode": mode})).await;
    }

    let Some(user) = app.auth.authenticate(
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok()),
    ) else {
        return reply(
            &mut stream,
            StatusCode::UNAUTHORIZED,
            json!({"error":"unauthorized"}),
        )
        .await;
    };

    let segments: Vec<_> = path.trim_matches('/').split('/').collect();
    match (method, segments.as_slice()) {
        ("GET", ["v1", "health"]) => {
            reply(
                &mut stream,
                StatusCode::OK,
                json!({"status":"ok","protocol":"iroh","version":1}),
            )
            .await
        }
        ("GET", ["v1", "notifications"]) => stream_notifications(&app, &user, &mut stream).await,
        ("GET", ["v1", "sessions"]) => {
            reply(
                &mut stream,
                StatusCode::OK,
                json!({"sessions": app.store.sessions(&user).await?}),
            )
            .await
        }
        ("GET", ["v1", "sessions", id]) => serve_conversation(&app, &user, id, &mut stream).await,
        ("PUT", ["v1", "sessions", id]) => {
            let Some(parsed) = read_json(&mut stream, 65536).await? else {
                return Ok(());
            };
            let Some(title) = parsed
                .get("title")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
            else {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"title\": \"...\"}"}),
                )
                .await;
            };
            let (status, body) =
                ok_or_not_found(app.store.rename_session(&user, id, title).await?, "renamed");
            reply(&mut stream, status, body).await
        }
        ("DELETE", ["v1", "sessions", id]) => {
            let (status, body) =
                ok_or_not_found(app.store.delete_session(&user, id).await?, "deleted");
            reply(&mut stream, status, body).await
        }
        ("PUT", ["v1", "sessions", id, "pin"]) => {
            let parsed = read_json(&mut stream, 4096).await?.unwrap_or_default();
            let pinned = parsed
                .get("pinned")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let (status, body) =
                ok_or_not_found(app.store.pin_session(&user, id, pinned).await?, "pinned");
            reply(&mut stream, status, body).await
        }
        ("GET", ["v1", "memories"]) => {
            reply(
                &mut stream,
                StatusCode::OK,
                json!({"memories": app.store.list_memories(&user).await?}),
            )
            .await
        }
        ("DELETE", ["v1", "memories"]) => {
            let Some(parsed) = read_json(&mut stream, 65536).await? else {
                return Ok(());
            };
            let Some(key) = parsed
                .get("key")
                .and_then(Value::as_str)
                .filter(|k| !k.is_empty())
            else {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"key\": \"...\"}"}),
                )
                .await;
            };
            let (status, body) = ok_or_not_found(app.store.forget(&user, key).await?, "forgotten");
            reply(&mut stream, status, body).await
        }
        ("GET", ["v1", "crons"]) => {
            let tz = app
                .store
                .get_timezone(&user)
                .await
                .unwrap_or_else(|_| "UTC".into());
            let mut crons = app.store.list_crons(&user).await?;
            for cron in &mut crons {
                let schedule = cron["schedule"].as_str().unwrap_or_default();
                cron["next_run_at"] = next_run_at(schedule, &tz)
                    .map(|t| json!(t))
                    .unwrap_or(Value::Null);
            }
            reply(&mut stream, StatusCode::OK, json!({"crons": crons})).await
        }
        ("POST", ["v1", "crons"]) => {
            let Some(parsed) = read_json(&mut stream, 65536).await? else {
                return Ok(());
            };
            let label = parsed.get("label").and_then(Value::as_str).unwrap_or("");
            let prompt = parsed.get("prompt").and_then(Value::as_str).unwrap_or("");
            let schedule = parsed.get("schedule").and_then(Value::as_str).unwrap_or("");
            match app.store.create_cron(&user, label, prompt, schedule).await {
                Ok(id) => reply(&mut stream, StatusCode::CREATED,
                    json!({"id":id,"label":label,"prompt":prompt,"schedule":schedule,"enabled":true})).await,
                Err(e) => reply(&mut stream, StatusCode::BAD_REQUEST, json!({"error": e.to_string()})).await,
            }
        }
        ("PUT", ["v1", "crons", id, "toggle"]) => match app.store.toggle_cron(&user, id).await? {
            Some(enabled) => reply(&mut stream, StatusCode::OK, json!({"enabled": enabled})).await,
            None => {
                reply(
                    &mut stream,
                    StatusCode::NOT_FOUND,
                    json!({"error":"cron not found"}),
                )
                .await
            }
        },
        ("DELETE", ["v1", "crons", id]) => {
            let (status, body) =
                ok_or_not_found(app.store.delete_cron(&user, id).await?, "deleted");
            reply(&mut stream, status, body).await
        }
        ("PATCH", ["v1", "crons", id]) => {
            let parsed = read_json(&mut stream, 65536).await?.unwrap_or_default();
            let enabled = parsed.get("enabled").and_then(Value::as_bool);
            let label = parsed.get("label").and_then(Value::as_str);
            let schedule = parsed.get("schedule").and_then(Value::as_str);
            let (status, body) = ok_or_not_found(
                app.store
                    .update_cron(&user, id, enabled, label, schedule)
                    .await?,
                "updated",
            );
            reply(&mut stream, status, body).await
        }
        ("POST", ["v1", "devices"]) => {
            let Some(parsed) = read_json(&mut stream, 4096).await? else {
                return Ok(());
            };
            let device_id = parsed
                .get("device_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let fcm_token = parsed
                .get("fcm_token")
                .and_then(Value::as_str)
                .unwrap_or("");
            if device_id.is_empty() || fcm_token.is_empty() {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"device_id\": \"...\", \"fcm_token\": \"...\"}"}),
                )
                .await;
            }
            app.store
                .register_device(&user, device_id, fcm_token)
                .await?;
            reply(&mut stream, StatusCode::OK, json!({"registered": true})).await
        }
        ("DELETE", ["v1", "devices"]) => {
            let Some(parsed) = read_json(&mut stream, 4096).await? else {
                return Ok(());
            };
            let Some(device_id) = parsed
                .get("device_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"device_id\": \"...\"}"}),
                )
                .await;
            };
            let (status, body) = ok_or_not_found(
                app.store.unregister_device(&user, device_id).await?,
                "unregistered",
            );
            reply(&mut stream, status, body).await
        }
        ("POST", ["v1", "location"]) => {
            let Some(parsed) = read_json(&mut stream, 4096).await? else {
                return Ok(());
            };
            let lat = parsed.get("latitude").and_then(Value::as_f64);
            let lng = parsed.get("longitude").and_then(Value::as_f64);
            let accuracy = parsed
                .get("accuracy")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            match (lat, lng) {
                (Some(lat), Some(lng))
                    if (-90.0..=90.0).contains(&lat)
                        && (-180.0..=180.0).contains(&lng)
                        && lat.is_finite()
                        && lng.is_finite()
                        && accuracy.is_finite() =>
                {
                    let tz = parsed
                        .get("timezone")
                        .and_then(Value::as_str)
                        .unwrap_or("UTC");
                    app.store
                        .update_location_tz(&user, lat, lng, accuracy, tz)
                        .await?;
                    reply(&mut stream, StatusCode::OK, json!({"updated": true})).await
                }
                _ => {
                    reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"expected valid latitude/longitude/accuracy"}),
                    )
                    .await
                }
            }
        }
        ("POST", ["v1", "phone", "notifications"]) => {
            let Some(parsed) = read_json(&mut stream, 262144).await? else {
                return Ok(());
            };
            let Some(notifications) = parsed.get("notifications").and_then(Value::as_array) else {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"notifications\": [...]}"}),
                )
                .await;
            };
            let saved = app.store.save_notifications(&user, notifications).await?;
            reply(&mut stream, StatusCode::OK, json!({"saved": saved})).await
        }
        ("POST", ["v1", "runs"]) => handle_create_run(&app, &user, &mut stream).await,
        ("GET", ["v1", "runs", id]) => match app.store.run(&user, id).await? {
            Some(run) => reply(&mut stream, StatusCode::OK, json!(run)).await,
            None => {
                reply(
                    &mut stream,
                    StatusCode::NOT_FOUND,
                    json!({"error":"run not found"}),
                )
                .await
            }
        },
        ("POST", ["v1", "runs", id, "cancel"]) => {
            if app.store.run(&user, id).await?.is_none() {
                return reply(
                    &mut stream,
                    StatusCode::NOT_FOUND,
                    json!({"error":"run not found"}),
                )
                .await;
            }
            app.cancel(id).await;
            reply(
                &mut stream,
                StatusCode::ACCEPTED,
                json!({"run_id": *id, "cancellation_requested": true}),
            )
            .await
        }
        ("POST", ["v1", "runs", id, "device-result"]) => {
            if app.store.run(&user, id).await?.is_none() {
                return reply(
                    &mut stream,
                    StatusCode::NOT_FOUND,
                    json!({"error":"run not found"}),
                )
                .await;
            }
            let Some(parsed) = read_json(&mut stream, 65536).await? else {
                return Ok(());
            };
            let Some(query_id) = parsed
                .get("query_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"query_id\": \"...\", \"result\": {...}}"}),
                )
                .await;
            };
            let result = parsed.get("result").cloned().unwrap_or(Value::Null);
            let resolved = app.resolve_device_query(query_id, result).await;
            let status = if resolved {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            };
            reply(&mut stream, status, json!({"resolved": resolved})).await
        }
        ("GET", ["v1", "runs", id, "events"]) => {
            if app.store.run(&user, id).await?.is_none() {
                return reply(
                    &mut stream,
                    StatusCode::NOT_FOUND,
                    json!({"error":"run not found"}),
                )
                .await;
            }
            stream_events(&app, &user, id, &request, &mut stream).await
        }
        ("GET", ["v1", "integrations"]) => {
            reply(
                &mut stream,
                StatusCode::OK,
                json!({"integrations": app.store.list_mcp_connections(&user).await?}),
            )
            .await
        }
        ("POST", ["v1", "integrations"]) => {
            let Some(parsed) = read_json(&mut stream, 65536).await? else {
                return Ok(());
            };
            let provider = parsed.get("provider").and_then(Value::as_str).unwrap_or("");
            let label = parsed.get("label").and_then(Value::as_str).unwrap_or("");
            let config = parsed
                .get("config")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".into());
            match app
                .store
                .create_mcp_connection(&user, provider, label, &config)
                .await
            {
                Ok(id) => {
                    reply(
                        &mut stream,
                        StatusCode::CREATED,
                        json!({"id":id,"provider":provider,"label":label,"status":"disconnected"}),
                    )
                    .await
                }
                Err(e) => {
                    reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error": e.to_string()}),
                    )
                    .await
                }
            }
        }
        ("DELETE", ["v1", "integrations", id]) => {
            let (status, body) =
                ok_or_not_found(app.store.delete_mcp_connection(&user, id).await?, "deleted");
            reply(&mut stream, status, body).await
        }
        ("PUT", ["v1", "integrations", id, "credentials"]) => {
            let Some(parsed) = read_json(&mut stream, 65536).await? else {
                return Ok(());
            };
            let credentials = parsed
                .get("credentials")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".into());
            let status_str = parsed
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("connected");
            let (status, body) = ok_or_not_found(
                app.store
                    .update_mcp_credentials(&user, id, &credentials, status_str)
                    .await?,
                "updated",
            );
            reply(&mut stream, status, body).await
        }
        _ => {
            reply(
                &mut stream,
                StatusCode::NOT_FOUND,
                json!({"error":"not found"}),
            )
            .await
        }
    }
}

async fn handle_create_run(app: &Arc<App>, user: &str, stream: &mut Stream) -> Result<()> {
    let Some(body) = read_body(stream, 5 * 1024 * 1024).await? else {
        return reply(
            stream,
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({"error":"body exceeds 5 MiB"}),
        )
        .await;
    };
    let message = match serde_json::from_slice::<Message>(&body) {
        Ok(m) if !m.message.trim().is_empty() && m.message.len() <= 32768 => m,
        _ => return reply(stream, StatusCode::BAD_REQUEST,
            json!({"error":"expected UUID session_id/request_id and a nonempty message up to 32 KiB"})).await,
    };
    match app.submit(user, message).await? {
        CreateRun::Created(run) => reply(stream, StatusCode::ACCEPTED, json!(run)).await,
        CreateRun::Existing(run) => reply(stream, StatusCode::OK, json!(run)).await,
        CreateRun::Conflict => {
            reply(
                stream,
                StatusCode::CONFLICT,
                json!({"error":"request ID conflict, session busy, or run capacity reached"}),
            )
            .await
        }
    }
}

async fn stream_notifications(app: &Arc<App>, user: &str, stream: &mut Stream) -> Result<()> {
    stream
        .send_response(
            Response::builder()
                .header("content-type", "application/x-ndjson")
                .header("cache-control", "no-store")
                .body(())?,
        )
        .await?;
    let mut rx = app.notifications.subscribe();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                let mut line = serde_json::to_vec(&json!({"type":"heartbeat"}))?;
                line.push(b'\n');
                if tokio::time::timeout(Duration::from_secs(10), stream.send_data(Bytes::from(line))).await.is_err() {
                    break;
                }
            }
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        let parsed: Value = match serde_json::from_str(&event) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        if parsed.get("user").and_then(Value::as_str) != Some(user) {
                            continue;
                        }
                        let mut line = Vec::from(event.as_bytes());
                        line.push(b'\n');
                        if tokio::time::timeout(Duration::from_secs(10), stream.send_data(Bytes::from(line))).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    stream.finish().await
}

async fn stream_events(
    app: &Arc<App>,
    user: &str,
    run_id: &str,
    request: &Request<()>,
    stream: &mut Stream,
) -> Result<()> {
    let mut after: i64 = match request.uri().query() {
        None => 0,
        Some(q) => match q
            .strip_prefix("after=")
            .and_then(|v| v.parse().ok())
            .filter(|v: &i64| *v >= 0)
        {
            Some(v) => v,
            None => {
                return reply(
                    stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected ?after=<nonnegative sequence>"}),
                )
                .await;
            }
        },
    };
    stream
        .send_response(
            Response::builder()
                .header("content-type", "application/x-ndjson")
                .header("cache-control", "no-store")
                .body(())?,
        )
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
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
            let terminal = event.data["type"] == "run_finished";
            let mut line = serde_json::to_vec(&event)?;
            line.push(b'\n');
            tokio::time::timeout(Duration::from_secs(10), stream.send_data(Bytes::from(line)))
                .await??;
            if terminal {
                return stream.finish().await;
            }
        }
        if (empty && !running) || tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stream.finish().await
}

async fn serve_conversation(
    app: &Arc<App>,
    user: &str,
    session_id: &str,
    stream: &mut Stream,
) -> Result<()> {
    match app.store.conversation(user, session_id).await? {
        Some(conversation) => {
            let entries: Vec<_> = conversation
                .entries()
                .iter()
                .filter_map(|entry| {
                    let (role, content) = match &entry.kind {
                        flashmind::core::EntryKind::User { content, .. } => ("user", content),
                        flashmind::core::EntryKind::Assistant { content, .. } => {
                            ("assistant", content)
                        }
                        _ => return None,
                    };
                    Some(json!({"role": role, "content": content, "timestamp": entry.timestamp}))
                })
                .collect();
            reply(
                stream,
                StatusCode::OK,
                json!({"session_id": session_id, "entries": entries}),
            )
            .await
        }
        None => {
            reply(
                stream,
                StatusCode::NOT_FOUND,
                json!({"error":"session not found"}),
            )
            .await
        }
    }
}
