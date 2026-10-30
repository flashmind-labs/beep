//! Composio trigger events without a public webhook: the server opens the
//! same realtime channel Composio's SDKs use for `triggers.subscribe()`.
//!
//! Composio hands out a Pusher key, cluster and project id at
//! `internal/sdk/realtime/credentials`. The server connects to Pusher, has
//! Composio sign the private channel `private-<project>_triggers` at
//! `internal/sdk/realtime/auth`, subscribes, and then gets every trigger
//! event of the project as `trigger_to_client`, or in pieces as
//! `chunked-trigger_to_client` when it is long. Each event names the
//! Composio user, which is the account's outside id. A dropped connection
//! is opened again with backoff.

use super::{
    enabled,
    normalize::{self, TriggerMessage},
    submit,
};
use crate::{app::App, apps::Composio};
use anyhow::{Context, Result, bail};
use futures::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message as Frame};

/// The realtime endpoints are on v3; v3.1 does not have them.
const REALTIME_URL: &str = "https://backend.composio.dev/api/v3/internal/sdk/realtime";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long Pusher's answer to a ping may take before the connection is
/// given up.
const PONG_TIMEOUT: Duration = Duration::from_secs(30);
/// Pusher's default when it does not say.
const ACTIVITY_TIMEOUT: Duration = Duration::from_secs(120);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(5 * 60);
/// A connection that lasted this long starts the backoff over.
const HEALTHY: Duration = Duration::from_secs(5 * 60);
/// Chunked events being put together at once, and the most one may hold.
const MAX_CHUNKED: usize = 32;
const MAX_CHUNKED_BYTES: usize = 4 * 1024 * 1024;

const TRIGGER_EVENT: &str = "trigger_to_client";
const CHUNKED_EVENT: &str = "chunked-trigger_to_client";

/// Keeps the channel open until the pipeline stops.
pub(crate) async fn run(app: Arc<App>, composio: Arc<Composio>) {
    let stop = app.events.stopping().clone();
    let mut retry = FIRST_RETRY;
    loop {
        let started = Instant::now();
        let session = tokio::select! {
            _ = stop.cancelled() => break,
            session = listen(&app, &composio) => session,
        };
        app.events.set_realtime(false);
        match session {
            Ok(()) => log::info!("events: realtime channel closed"),
            Err(error) => log::warn!("events: realtime channel failed: {error:#}"),
        }
        if started.elapsed() >= HEALTHY {
            retry = FIRST_RETRY;
        }
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = sleep(retry) => {}
        }
        retry = (retry * 2).min(MAX_RETRY);
    }
    app.events.set_realtime(false);
}

/// What Composio gives out for the channel.
#[derive(Deserialize)]
struct Credentials {
    pusher_key: String,
    project_id: String,
    pusher_cluster: String,
}

#[derive(Deserialize)]
struct ChannelAuth {
    auth: String,
    /// Composio signs the channel as a presence channel: the signature
    /// covers this too, so the subscribe must carry it unchanged.
    #[serde(default)]
    channel_data: Option<String>,
}

/// The frame that subscribes to `channel` with Composio's signature.
fn subscribe_frame(channel: &str, auth: ChannelAuth) -> Value {
    let mut data = json!({"auth": auth.auth, "channel": channel});
    if let Some(channel_data) = auth.channel_data {
        data["channel_data"] = Value::String(channel_data);
    }
    json!({"event": "pusher:subscribe", "data": data})
}

/// One connection, from credentials to the moment it drops.
async fn listen(app: &App, composio: &Composio) -> Result<()> {
    let key = composio.settings.key.expose_secret();
    let credentials: Credentials = composio
        .http
        .get(format!("{REALTIME_URL}/credentials"))
        .header("x-api-key", key)
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .context("credentials request failed")?
        .error_for_status()
        .context("credentials refused")?
        .json()
        .await
        .context("invalid credentials")?;
    let url = pusher_url(&credentials)?;
    let channel = format!("private-{}_triggers", credentials.project_id);
    let (mut socket, _) = timeout(CONNECT_TIMEOUT, connect_async(url.as_str()))
        .await
        .context("connecting timed out")?
        .context("connecting failed")?;

    // Pusher opens with the socket id the channel auth signs.
    let established = timeout(CONNECT_TIMEOUT, next_frame(&mut socket))
        .await
        .context("no greeting from Pusher")??;
    if established.event != "pusher:connection_established" {
        bail!("Pusher opened with {}", established.event);
    }
    let greeting = established.data();
    let socket_id = greeting["socket_id"]
        .as_str()
        .context("Pusher sent no socket id")?
        .to_owned();
    let activity = greeting["activity_timeout"]
        .as_u64()
        .map_or(ACTIVITY_TIMEOUT, Duration::from_secs);

    let auth: ChannelAuth = composio
        .http
        .post(format!("{REALTIME_URL}/auth"))
        .header("x-api-key", key)
        .form(&[
            ("socket_id", socket_id.as_str()),
            ("channel_name", &channel),
        ])
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .context("channel auth request failed")?
        .error_for_status()
        .context("channel auth refused")?
        .json()
        .await
        .context("invalid channel auth")?;
    let subscribe = subscribe_frame(&channel, auth);
    socket.send(Frame::text(subscribe.to_string())).await?;

    let mut chunks = Chunks::default();
    let mut ping_sent: Option<Instant> = None;
    loop {
        let wait = match ping_sent {
            Some(sent) => PONG_TIMEOUT.saturating_sub(sent.elapsed()),
            None => activity,
        };
        let frame = match timeout(wait, socket.next()).await {
            Ok(Some(frame)) => frame.context("reading from Pusher")?,
            Ok(None) => return Ok(()),
            Err(_) if ping_sent.is_some() => bail!("Pusher stopped answering"),
            Err(_) => {
                let ping = json!({"event": "pusher:ping", "data": {}});
                socket.send(Frame::text(ping.to_string())).await?;
                ping_sent = Some(Instant::now());
                continue;
            }
        };
        ping_sent = None;
        let text = match frame {
            Frame::Text(text) => text,
            Frame::Close(_) => return Ok(()),
            // tungstenite answers websocket pings itself.
            _ => continue,
        };
        let Ok(frame) = serde_json::from_str::<PusherFrame>(&text) else {
            log::debug!("events: unreadable Pusher frame");
            continue;
        };
        match frame.event.as_str() {
            "pusher:ping" => {
                let pong = json!({"event": "pusher:pong", "data": {}});
                socket.send(Frame::text(pong.to_string())).await?;
            }
            "pusher_internal:subscription_succeeded" => {
                log::info!("events: realtime channel subscribed");
                app.events.set_realtime(true);
            }
            "pusher:subscription_error" | "pusher:error" => {
                bail!("Pusher refused: {}", frame.data())
            }
            TRIGGER_EVENT => deliver(app, &frame.data()).await,
            CHUNKED_EVENT => {
                if let Some(payload) = chunks.add(&frame.data()) {
                    deliver(app, &payload).await;
                }
            }
            _ => {}
        }
    }
}

/// The websocket address for `credentials`, refusing a cluster that is not
/// a plain name.
fn pusher_url(credentials: &Credentials) -> Result<String> {
    let plain = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if !plain(&credentials.pusher_cluster) || !plain(&credentials.pusher_key) {
        bail!("unexpected Pusher credentials");
    }
    Ok(format!(
        "wss://ws-{}.pusher.com/app/{}?protocol=7&client=beep-server&version=1.0&flash=false",
        credentials.pusher_cluster, credentials.pusher_key
    ))
}

async fn next_frame<S>(socket: &mut S) -> Result<PusherFrame>
where
    S: StreamExt<Item = Result<Frame, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(frame) = socket.next().await {
        if let Frame::Text(text) = frame? {
            return serde_json::from_str(&text).context("unreadable Pusher frame");
        }
    }
    bail!("Pusher closed the connection")
}

/// A Pusher protocol message.
#[derive(Debug, Deserialize)]
struct PusherFrame {
    event: String,
    #[serde(default)]
    data: Value,
}

impl PusherFrame {
    /// The data, which Pusher usually sends as a JSON string.
    fn data(&self) -> Value {
        match &self.data {
            Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
            other => other.clone(),
        }
    }
}

/// Puts chunked events back together.
#[derive(Default)]
struct Chunks {
    open: HashMap<String, Chunked>,
}

#[derive(Default)]
struct Chunked {
    parts: Vec<Option<String>>,
    last: Option<usize>,
    bytes: usize,
}

impl Chunks {
    /// Takes one `{id, index, chunk, final}` piece, and returns the whole
    /// payload once every piece is in.
    fn add(&mut self, piece: &Value) -> Option<Value> {
        let id = piece["id"].as_str()?;
        let index = usize::try_from(piece["index"].as_u64()?).ok()?;
        let chunk = piece["chunk"].as_str()?;
        if !self.open.contains_key(id) && self.open.len() >= MAX_CHUNKED {
            log::warn!("events: too many chunked events at once; one dropped");
            return None;
        }
        let entry = self.open.entry(id.to_owned()).or_default();
        entry.bytes += chunk.len();
        if entry.bytes > MAX_CHUNKED_BYTES || index > MAX_CHUNKED_BYTES {
            log::warn!("events: chunked event too large; dropped");
            self.open.remove(id);
            return None;
        }
        if entry.parts.len() <= index {
            entry.parts.resize(index + 1, None);
        }
        entry.parts[index] = Some(chunk.to_owned());
        if piece["final"].as_bool() == Some(true) {
            entry.last = Some(index);
        }
        let last = entry.last?;
        if entry.parts.len() != last + 1 || entry.parts.iter().any(Option::is_none) {
            return None;
        }
        let whole: String = self.open.remove(id)?.parts.into_iter().flatten().collect();
        serde_json::from_str(&whole)
            .inspect_err(|error| log::warn!("events: unreadable chunked event: {error}"))
            .ok()
    }
}

/// Hands one trigger payload to the pipeline, for the user it names.
pub async fn deliver(app: &App, payload: &Value) {
    let Some(message) = TriggerMessage::parse(payload) else {
        log::debug!("events: trigger payload in an unknown shape");
        return;
    };
    let Some(user) = app
        .users()
        .into_iter()
        .find(|user| user.external_id() == message.user)
    else {
        // The project's channel carries every server's users.
        log::debug!("events: trigger for a user this server does not have");
        return;
    };
    if !enabled(app, &user) {
        return;
    }
    match normalize::from_trigger(&message) {
        Some(event) => submit(app, user.id(), event).await,
        None => log::debug!(
            "events: trigger {} made no event; user={user}",
            message.slug
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_read_their_data_as_json_text_or_as_is() {
        let text: PusherFrame = serde_json::from_str(
            r#"{"event":"pusher:connection_established","data":"{\"socket_id\":\"1.2\",\"activity_timeout\":120}"}"#,
        )
        .unwrap();
        assert_eq!(text.data()["socket_id"], "1.2");
        let object: PusherFrame =
            serde_json::from_str(r#"{"event":"pusher:error","data":{"code":4001}}"#).unwrap();
        assert_eq!(object.data()["code"], 4001);
    }

    #[test]
    fn chunked_events_come_out_whole_in_any_order() {
        let payload = json!({"metadata": {"trigger_slug": "X", "user_id": "u"}, "data": {"a": 1}});
        let text = payload.to_string();
        let (head, tail) = text.split_at(text.len() / 2);
        let mut chunks = Chunks::default();
        assert!(
            chunks
                .add(&json!({"id": "e1", "index": 1, "chunk": tail, "final": true}))
                .is_none()
        );
        let whole = chunks
            .add(&json!({"id": "e1", "index": 0, "chunk": head, "final": false}))
            .unwrap();
        assert_eq!(whole, payload);
        assert!(chunks.open.is_empty());
    }

    #[test]
    fn chunked_events_are_bounded() {
        let mut chunks = Chunks::default();
        for n in 0..MAX_CHUNKED {
            let piece = json!({"id": format!("e{n}"), "index": 0, "chunk": "{", "final": false});
            assert!(chunks.add(&piece).is_none());
        }
        let extra = json!({"id": "extra", "index": 0, "chunk": "{}", "final": true});
        assert!(chunks.add(&extra).is_none());
        assert!(!chunks.open.contains_key("extra"));
    }

    #[test]
    fn the_socket_url_takes_only_plain_credentials() {
        let credentials = |cluster: &str| Credentials {
            pusher_key: "abc123".into(),
            project_id: "p".into(),
            pusher_cluster: cluster.into(),
        };
        assert_eq!(
            pusher_url(&credentials("mt1")).unwrap(),
            "wss://ws-mt1.pusher.com/app/abc123?protocol=7&client=beep-server&version=1.0&flash=false"
        );
        assert!(pusher_url(&credentials("evil.example.com/x")).is_err());
    }

    #[test]
    fn the_subscribe_carries_the_signed_channel_data() {
        let auth: ChannelAuth =
            serde_json::from_str(r#"{"auth":"key:sig","channel_data":"{\"user_id\":\"p1\"}"}"#)
                .expect("Composio's auth reply");
        let frame = subscribe_frame("private-p1_triggers", auth);
        assert_eq!(frame["event"], "pusher:subscribe");
        assert_eq!(frame["data"]["auth"], "key:sig");
        assert_eq!(frame["data"]["channel_data"], r#"{"user_id":"p1"}"#);

        let auth: ChannelAuth =
            serde_json::from_str(r#"{"auth":"key:sig"}"#).expect("a plain private auth");
        let frame = subscribe_frame("private-p1_triggers", auth);
        assert!(frame["data"].get("channel_data").is_none());
    }
}
