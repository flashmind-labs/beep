use crate::{
    app::{App, Message},
    store::CreateRun,
};
use anyhow::{Context, Result};
use bytes::{Buf, Bytes, BytesMut};
use http::{Request, Response, StatusCode};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{Value, json};
use std::{io::BufReader, net::SocketAddr, path::Path, sync::Arc, time::Duration};

type QuicStream = h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;
pub(crate) enum Stream {
    Quic(Box<QuicStream>),
    Http {
        body: hyper::body::Incoming,
        headers: Option<tokio::sync::oneshot::Sender<Response<()>>>,
        data: tokio::sync::mpsc::Sender<Bytes>,
    },
}
impl Stream {
    async fn recv_data(&mut self) -> Result<Option<Bytes>> {
        match self {
            Self::Quic(stream) => Ok(stream
                .recv_data()
                .await?
                .map(|mut b| b.copy_to_bytes(b.remaining()))),
            Self::Http { body, .. } => {
                use http_body_util::BodyExt;
                while let Some(frame) = body.frame().await {
                    if let Ok(bytes) = frame?.into_data() {
                        return Ok(Some(bytes));
                    }
                }
                Ok(None)
            }
        }
    }
    async fn send_response(&mut self, response: Response<()>) -> Result<()> {
        match self {
            Self::Quic(stream) => stream.send_response(response).await?,
            Self::Http { headers, .. } => {
                headers
                    .take()
                    .context("headers already sent")?
                    .send(response)
                    .map_err(|_| anyhow::anyhow!("client disconnected"))?;
            }
        }
        Ok(())
    }
    async fn send_data(&mut self, bytes: Bytes) -> Result<()> {
        match self {
            Self::Quic(stream) => stream.send_data(bytes).await?,
            Self::Http { data, .. } => data
                .send(bytes)
                .await
                .map_err(|_| anyhow::anyhow!("client disconnected"))?,
        }
        Ok(())
    }
    async fn finish(&mut self) -> Result<()> {
        if let Self::Quic(stream) = self {
            stream.finish().await?;
        }
        Ok(())
    }
}

pub fn certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(std::fs::File::open(path)?);
    let certs = rustls_pemfile::certs(&mut reader).collect::<std::io::Result<Vec<_>>>()?;
    anyhow::ensure!(!certs.is_empty(), "no certificates in PEM file");
    Ok(certs)
}

pub fn private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    rustls_pemfile::private_key(&mut BufReader::new(std::fs::File::open(path)?))?
        .context("no private key in PEM file")
}

pub fn endpoint(
    addr: SocketAddr,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::Endpoint> {
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    tls.alpn_protocols = vec![b"h3".to_vec()];
    // No 0-RTT: authenticated mutations must not be replayable as TLS early data.
    tls.max_early_data_size = 0;
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(32u32.into());
    transport.max_idle_timeout(Some(Duration::from_secs(60).try_into()?));
    config.transport_config(Arc::new(transport));
    Ok(quinn::Endpoint::server(config, addr)?)
}

pub async fn serve(endpoint: quinn::Endpoint, app: Arc<App>) -> Result<()> {
    // Bound unauthenticated connection work as well as authenticated run concurrency.
    let connections = Arc::new(tokio::sync::Semaphore::new(128));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(permit) = connections.clone().try_acquire_owned() else {
            incoming.refuse();
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result: Result<()> = async {
                let connection = tokio::time::timeout(Duration::from_secs(10), incoming).await??;
                let mut h3 =
                    h3::server::Connection::new(h3_quinn::Connection::new(connection)).await?;
                while let Some(resolver) = h3.accept().await? {
                    let app = app.clone();
                    tokio::spawn(async move {
                        let result: Result<()> = async {
                            let (request, stream) = tokio::time::timeout(
                                Duration::from_secs(10),
                                resolver.resolve_request(),
                            )
                            .await??;
                            handle(app, request, Stream::Quic(Box::new(stream))).await
                        }
                        .await;
                        if let Err(error) = result {
                            tracing::debug!(%error,"HTTP/3 request ended");
                        }
                    });
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                tracing::debug!(%error,"QUIC connection ended");
            }
        });
    }
    Ok(())
}

async fn read_body(stream: &mut Stream, limit: usize) -> Result<Option<Bytes>> {
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut body = BytesMut::new();
        while let Some(mut chunk) = stream.recv_data().await? {
            if body.len() + chunk.remaining() > limit {
                return Ok::<_, anyhow::Error>(None);
            }
            body.extend_from_slice(&chunk.copy_to_bytes(chunk.remaining()));
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
            return reply(
                &mut stream,
                StatusCode::OK,
                json!({"crons":app.store.list_crons(&user).await?}),
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
            let label = parsed.get("label").and_then(Value::as_str).unwrap_or("");
            let prompt = parsed.get("prompt").and_then(Value::as_str).unwrap_or("");
            let schedule = parsed
                .get("schedule")
                .and_then(Value::as_str)
                .unwrap_or("");
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
            let device_id = parsed.get("device_id").and_then(Value::as_str).unwrap_or("");
            let fcm_token = parsed.get("fcm_token").and_then(Value::as_str).unwrap_or("");
            if device_id.is_empty() || fcm_token.is_empty() {
                return reply(
                    &mut stream,
                    StatusCode::BAD_REQUEST,
                    json!({"error":"expected {\"device_id\": \"...\", \"fcm_token\": \"...\"}"}),
                )
                .await;
            }
            app.store.register_device(&user, device_id, fcm_token).await?;
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
            return reply(&mut stream, StatusCode::OK, json!({"unregistered":unregistered})).await;
        }
    }
    if method == "GET" && path == "/v1/health" {
        let protocol = if matches!(stream, Stream::Quic(_)) {
            "h3"
        } else {
            "http"
        };
        return reply(
            &mut stream,
            StatusCode::OK,
            json!({"status":"ok","protocol":protocol,"version":1}),
        )
        .await;
    }
    if method == "POST" && path == "/v1/runs" {
        let body = read_body(&mut stream, 65536).await?;
        let Some(body) = body else {
            return reply(
                &mut stream,
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error":"body exceeds 64 KiB"}),
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
    if segments.len() == 3 && segments[0] == "v1" && segments[1] == "crons" {
        let cron_id = segments[2];
        if method == "DELETE" {
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
        if method == "PATCH" {
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
            let deleted = app.store.delete_mcp_connection(&user, integration_id).await?;
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
