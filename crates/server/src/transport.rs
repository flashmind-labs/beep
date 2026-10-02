use crate::{
    app::{App, Message},
    store::CreateRun,
};
use anyhow::Result;
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use cron::Schedule;
use http::{Request, Response, StatusCode};
use iroh::endpoint::Connection;
use iroh::protocol::AcceptError;
use serde_json::{Value, json};
use std::{str::FromStr, sync::Arc, time::Duration};

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
                    tracing::debug!(%error, "iroh request ended");
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

/// Compute the next fire time for a 5-field cron expression in the given timezone.
fn next_run_at(expression: &str, tz_name: &str) -> Option<String> {
    let seven_field = format!("0 {} *", expression);
    let schedule = Schedule::from_str(&seven_field).ok()?;
    let tz: chrono_tz::Tz = tz_name.parse().unwrap_or(chrono_tz::UTC);
    let now_local = Utc::now().with_timezone(&tz);
    let next = schedule.after(&now_local).next()?;
    Some(next.with_timezone(&Utc).to_rfc3339())
}

pub(crate) async fn handle(app: Arc<App>, request: Request<()>, mut stream: Stream) -> Result<()> {
    if request.method() == "GET" && request.uri().path() == "/v1/auth" {
        return reply(
            &mut stream,
            StatusCode::OK,
            json!({"mode":if app.auth.is_testing() {"testing"} else {"token"}}),
        )
        .await;
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
    let path = request.uri().path();
    let method = request.method().as_str();
    if method == "GET" && path == "/v1/notifications" {
        stream
            .send_response(
                Response::builder()
                    .header("content-type", "application/x-ndjson")
                    .header("cache-control", "no-store")
                    .body(())?,
            )
            .await?;
        let mut rx = app.notifications.subscribe();
        let user = user.to_owned();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    let mut line = serde_json::to_vec(&json!({"type":"heartbeat"}))?;
                    line.push(b'\n');
                    if tokio::time::timeout(
                        Duration::from_secs(10),
                        stream.send_data(Bytes::from(line)),
                    ).await.is_err() {
                        break;
                    }
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            // Only forward notifications for the authenticated user.
                            let parsed: Value = match serde_json::from_str(&event) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            if parsed.get("user").and_then(Value::as_str) != Some(&user) {
                                continue;
                            }
                            let mut line = Vec::from(event.as_bytes());
                            line.push(b'\n');
                            match tokio::time::timeout(
                                Duration::from_secs(10),
                                stream.send_data(Bytes::from(line)),
                            ).await {
                                Ok(Ok(())) => {}
                                _ => break,
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
        stream.finish().await?;
        return Ok(());
    }
    if method == "GET" && path == "/v1/sessions" {
        return reply(
            &mut stream,
            StatusCode::OK,
            json!({"sessions":app.store.sessions(&user).await?}),
        )
        .await;
    }
    if path == "/v1/memories" {
        if method == "GET" {
            return reply(
                &mut stream,
                StatusCode::OK,
                json!({"memories":app.store.list_memories(&user).await?}),
            )
            .await;
        }
        if method == "DELETE" {
            let body = read_body(&mut stream, 65536).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 64 KiB"}),
                )
                .await;
            };
            let key = match serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v.get("key")?.as_str().map(|s| s.to_owned()))
            {
                Some(key) if !key.is_empty() => key,
                _ => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"expected {\"key\": \"...\"}"}),
                    )
                    .await;
                }
            };
            let forgotten = app.store.forget(&user, &key).await?;
            return reply(
                &mut stream,
                if forgotten {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"forgotten":forgotten}),
            )
            .await;
        }
    }
    if path == "/v1/crons" {
        if method == "GET" {
            let tz = app
                .store
                .get_timezone(&user)
                .await
                .unwrap_or_else(|_| "UTC".into());
            let mut crons = app.store.list_crons(&user).await?;
            for cron in &mut crons {
                let schedule = cron["schedule"].as_str().unwrap_or_default();
                cron["next_run_at"] = match next_run_at(schedule, &tz) {
                    Some(t) => json!(t),
                    None => Value::Null,
                };
            }
            return reply(&mut stream, StatusCode::OK, json!({"crons": crons})).await;
        }
        if method == "POST" {
            let body = read_body(&mut stream, 65536).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 64 KiB"}),
                )
                .await;
            };
            let parsed: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"invalid JSON"}),
                    )
                    .await;
                }
            };
            let label = parsed.get("label").and_then(Value::as_str).unwrap_or("");
            let prompt = parsed.get("prompt").and_then(Value::as_str).unwrap_or("");
            let schedule = parsed.get("schedule").and_then(Value::as_str).unwrap_or("");
            match app.store.create_cron(&user, label, prompt, schedule).await {
                Ok(id) => {
                    return reply(
                        &mut stream,
                        StatusCode::CREATED,
                        json!({"id":id,"label":label,"prompt":prompt,"schedule":schedule,"enabled":true}),
                    )
                    .await;
                }
                Err(error) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":error.to_string()}),
                    )
                    .await;
                }
            }
        }
    }
    if path == "/v1/devices" {
        if method == "POST" {
            let body = read_body(&mut stream, 4096).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 4 KiB"}),
                )
                .await;
            };
            let parsed: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"invalid JSON"}),
                    )
                    .await;
                }
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
            return reply(&mut stream, StatusCode::OK, json!({"registered":true})).await;
        }
        if method == "DELETE" {
            let body = read_body(&mut stream, 4096).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 4 KiB"}),
                )
                .await;
            };
            let device_id = match serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v.get("device_id")?.as_str().map(|s| s.to_owned()))
            {
                Some(id) if !id.is_empty() => id,
                _ => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"expected {\"device_id\": \"...\"}"}),
                    )
                    .await;
                }
            };
            let unregistered = app.store.unregister_device(&user, &device_id).await?;
            return reply(
                &mut stream,
                StatusCode::OK,
                json!({"unregistered":unregistered}),
            )
            .await;
        }
    }
    if method == "POST" && path == "/v1/location" {
        let body = read_body(&mut stream, 4096).await?;
        let Some(body) = body else {
            return reply(
                &mut stream,
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error":"body exceeds 4 KiB"}),
            )
            .await;
        };
        let parsed: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"invalid JSON"}),
                )
                .await;
            }
        };
        let latitude = parsed.get("latitude").and_then(Value::as_f64);
        let longitude = parsed.get("longitude").and_then(Value::as_f64);
        let accuracy = parsed
            .get("accuracy")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        match (latitude, longitude) {
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
                return reply(&mut stream, StatusCode::OK, json!({"updated":true})).await;
            }
            _ => {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"latitude\": f64, \"longitude\": f64, \"accuracy\": f64}"}),
                )
                .await;
            }
        }
    }
    if method == "POST" && path == "/v1/phone/notifications" {
        let body = read_body(&mut stream, 262144).await?;
        let Some(body) = body else {
            return reply(
                &mut stream,
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error":"body exceeds 256 KiB"}),
            )
            .await;
        };
        let parsed: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"invalid JSON"}),
                )
                .await;
            }
        };
        let notifications = match parsed.get("notifications").and_then(Value::as_array) {
            Some(arr) => arr.clone(),
            None => {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"notifications\": [...]}"}),
                )
                .await;
            }
        };
        let saved = app.store.save_notifications(&user, &notifications).await?;
        return reply(&mut stream, StatusCode::OK, json!({"saved": saved})).await;
    }
    if method == "GET" && path == "/v1/health" {
        return reply(
            &mut stream,
            StatusCode::OK,
            json!({"status":"ok","protocol":"iroh","version":1}),
        )
        .await;
    }
    if method == "POST" && path == "/v1/runs" {
        // 5 MiB to accommodate base64-encoded image attachments.
        let body = read_body(&mut stream, 5 * 1024 * 1024).await?;
        let Some(body) = body else {
            return reply(
                &mut stream,
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error":"body exceeds 5 MiB"}),
            )
            .await;
        };
        let message = match serde_json::from_slice::<Message>(&body) {
            Ok(message) if !message.message.trim().is_empty() && message.message.len()<=32768 => message,
            _ => return reply(&mut stream,StatusCode::BAD_REQUEST,json!({"error":"expected UUID session_id/request_id and a nonempty message up to 32 KiB"})).await,
        };
        return match app.submit(&user, message).await? {
            CreateRun::Created(run) => reply(&mut stream, StatusCode::ACCEPTED, json!(run)).await,
            CreateRun::Existing(run) => reply(&mut stream, StatusCode::OK, json!(run)).await,
            CreateRun::Conflict => {
                reply(
                    &mut stream,
                    StatusCode::CONFLICT,
                    json!({"error":"request ID conflict, session busy, or run capacity reached"}),
                )
                .await
            }
        };
    }
    let segments: Vec<_> = path.trim_matches('/').split('/').collect();
    if segments.len() >= 3 && segments[0] == "v1" && segments[1] == "runs" {
        let Some(run) = app.store.run(&user, segments[2]).await? else {
            return reply(
                &mut stream,
                StatusCode::NOT_FOUND,
                json!({"error":"run not found"}),
            )
            .await;
        };
        if method == "GET" && segments.len() == 3 {
            return reply(&mut stream, StatusCode::OK, json!(run)).await;
        }
        if method == "POST" && segments.get(3) == Some(&"cancel") && segments.len() == 4 {
            app.cancel(&run.id).await;
            return reply(
                &mut stream,
                StatusCode::ACCEPTED,
                json!({"run_id":run.id,"cancellation_requested":true}),
            )
            .await;
        }
        if method == "POST" && segments.get(3) == Some(&"device-result") && segments.len() == 4 {
            let body = read_body(&mut stream, 65536).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 64 KiB"}),
                )
                .await;
            };
            let parsed: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"invalid JSON"}),
                    )
                    .await;
                }
            };
            let query_id = match parsed.get("query_id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() => id,
                _ => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"expected {\"query_id\": \"...\", \"result\": {...}}"}),
                    )
                    .await;
                }
            };
            let result = parsed.get("result").cloned().unwrap_or(Value::Null);
            let resolved = app.resolve_device_query(query_id, result).await;
            return reply(
                &mut stream,
                if resolved {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"resolved": resolved}),
            )
            .await;
        }
        if method == "GET" && segments.get(3) == Some(&"events") && segments.len() == 4 {
            let mut after = match request.uri().query() {
                None => 0,
                Some(query) => match query
                    .strip_prefix("after=")
                    .and_then(|v| v.parse::<i64>().ok())
                    .filter(|v| *v >= 0)
                {
                    Some(value) => value,
                    None => {
                        return reply(
                            &mut stream,
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
            // Bounded subscription. Clients reconnect with their last durable sequence.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
            loop {
                // Read status first: otherwise completion between the event query and
                // status query could cause us to close without sending the terminal event.
                let running = app
                    .store
                    .run(&user, &run.id)
                    .await?
                    .is_some_and(|run| run.status == "running");
                let events = app.store.events(&run.id, after).await?;
                let empty = events.is_empty();
                for event in events {
                    after = event.seq;
                    let terminal = event.data["type"] == "run_finished";
                    let mut line = serde_json::to_vec(&event)?;
                    line.push(b'\n');
                    tokio::time::timeout(
                        Duration::from_secs(10),
                        stream.send_data(Bytes::from(line)),
                    )
                    .await??;
                    if terminal {
                        stream.finish().await?;
                        return Ok(());
                    }
                }
                if empty && !running {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            stream.finish().await?;
            return Ok(());
        }
    }
    if segments.len() >= 3 && segments[0] == "v1" && segments[1] == "crons" {
        let cron_id = segments[2];
        if method == "PUT" && segments.len() == 4 && segments[3] == "toggle" {
            return match app.store.toggle_cron(&user, cron_id).await? {
                Some(enabled) => {
                    reply(&mut stream, StatusCode::OK, json!({"enabled": enabled})).await
                }
                None => {
                    reply(
                        &mut stream,
                        StatusCode::NOT_FOUND,
                        json!({"error":"cron not found"}),
                    )
                    .await
                }
            };
        }
        if method == "DELETE" && segments.len() == 3 {
            let deleted = app.store.delete_cron(&user, cron_id).await?;
            return reply(
                &mut stream,
                if deleted {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"deleted":deleted}),
            )
            .await;
        }
        if method == "PATCH" && segments.len() == 3 {
            let body = read_body(&mut stream, 65536).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 64 KiB"}),
                )
                .await;
            };
            let parsed: Value = serde_json::from_slice(&body).unwrap_or_default();
            let enabled = parsed.get("enabled").and_then(Value::as_bool);
            let label = parsed.get("label").and_then(Value::as_str);
            let schedule = parsed.get("schedule").and_then(Value::as_str);
            let updated = app
                .store
                .update_cron(&user, cron_id, enabled, label, schedule)
                .await?;
            return reply(
                &mut stream,
                if updated {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"updated":updated}),
            )
            .await;
        }
    }
    if path == "/v1/integrations" {
        if method == "GET" {
            return reply(
                &mut stream,
                StatusCode::OK,
                json!({"integrations":app.store.list_mcp_connections(&user).await?}),
            )
            .await;
        }
        if method == "POST" {
            let body = read_body(&mut stream, 65536).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 64 KiB"}),
                )
                .await;
            };
            let parsed: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"invalid JSON"}),
                    )
                    .await;
                }
            };
            let provider = parsed.get("provider").and_then(Value::as_str).unwrap_or("");
            let label = parsed.get("label").and_then(Value::as_str).unwrap_or("");
            let config = parsed
                .get("config")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".to_string());
            match app
                .store
                .create_mcp_connection(&user, provider, label, &config)
                .await
            {
                Ok(id) => {
                    return reply(
                        &mut stream,
                        StatusCode::CREATED,
                        json!({"id":id,"provider":provider,"label":label,"status":"disconnected"}),
                    )
                    .await;
                }
                Err(error) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":error.to_string()}),
                    )
                    .await;
                }
            }
        }
    }
    if segments.len() >= 3 && segments[0] == "v1" && segments[1] == "integrations" {
        let integration_id = segments[2];
        if method == "DELETE" && segments.len() == 3 {
            let deleted = app
                .store
                .delete_mcp_connection(&user, integration_id)
                .await?;
            return reply(
                &mut stream,
                if deleted {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"deleted":deleted}),
            )
            .await;
        }
        if method == "PUT" && segments.len() == 4 && segments[3] == "credentials" {
            let body = read_body(&mut stream, 65536).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 64 KiB"}),
                )
                .await;
            };
            let parsed: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"invalid JSON"}),
                    )
                    .await;
                }
            };
            let credentials = parsed
                .get("credentials")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".to_string());
            let status = parsed
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("connected");
            let updated = app
                .store
                .update_mcp_credentials(&user, integration_id, &credentials, status)
                .await?;
            return reply(
                &mut stream,
                if updated {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"updated":updated}),
            )
            .await;
        }
    }
    if segments.len() >= 3 && segments[0] == "v1" && segments[1] == "sessions" {
        let session_id = segments[2];
        if method == "PUT" && segments.len() == 3 {
            let body = read_body(&mut stream, 65536).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 64 KiB"}),
                )
                .await;
            };
            let parsed: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"invalid JSON"}),
                    )
                    .await;
                }
            };
            let title = match parsed.get("title").and_then(Value::as_str) {
                Some(t) if !t.is_empty() => t,
                _ => {
                    return reply(
                        &mut stream,
                        StatusCode::BAD_REQUEST,
                        json!({"error":"expected {\"title\": \"...\"}"}),
                    )
                    .await;
                }
            };
            let renamed = app.store.rename_session(&user, session_id, title).await?;
            return reply(
                &mut stream,
                if renamed {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"renamed":renamed}),
            )
            .await;
        }
        if method == "DELETE" && segments.len() == 3 {
            let deleted = app.store.delete_session(&user, session_id).await?;
            return reply(
                &mut stream,
                if deleted {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"deleted":deleted}),
            )
            .await;
        }
        if method == "PUT" && segments.len() == 4 && segments[3] == "pin" {
            let body = read_body(&mut stream, 4096).await?;
            let Some(body) = body else {
                return reply(
                    &mut stream,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"body exceeds 4 KiB"}),
                )
                .await;
            };
            let parsed: Value = serde_json::from_slice(&body).unwrap_or_default();
            let pinned = parsed
                .get("pinned")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let updated = app.store.pin_session(&user, session_id, pinned).await?;
            return reply(
                &mut stream,
                if updated {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                json!({"pinned":pinned}),
            )
            .await;
        }
    }
    if method == "GET" && segments.len() == 3 && segments[0] == "v1" && segments[1] == "sessions" {
        // Return conversation entries only: never send the server system prompt to a client.
        return match app.store.conversation(&user, segments[2]).await? {
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
                        Some(json!({"role":role,"content":content,"timestamp":entry.timestamp}))
                    })
                    .collect();
                reply(
                    &mut stream,
                    StatusCode::OK,
                    json!({"session_id":segments[2],"entries":entries}),
                )
                .await
            }
            None => {
                reply(
                    &mut stream,
                    StatusCode::NOT_FOUND,
                    json!({"error":"session not found"}),
                )
                .await
            }
        };
    }
    reply(
        &mut stream,
        StatusCode::NOT_FOUND,
        json!({"error":"not found"}),
    )
    .await
}
