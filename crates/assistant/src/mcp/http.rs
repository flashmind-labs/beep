//! A server reached over streamable HTTP: each message is a POST, and the answer is either one
//! JSON body or a short event stream that ends with the answer.

use super::McpError;
use futures::StreamExt;
use reqwest::{Client, Url, header};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

/// Most bytes read of one answer.
const MAX_BODY: usize = 4 * 1024 * 1024;

pub(super) struct Http {
    client: Client,
    url: Url,
    token: Option<SecretString>,
    last_id: AtomicU64,
    session: Mutex<Session>,
}

/// What the server fixed in its answer to `initialize`.
#[derive(Default)]
struct Session {
    id: Option<String>,
    version: Option<String>,
}

impl Http {
    pub(super) fn new(url: Url, token: Option<SecretString>) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            client,
            url,
            token,
            last_id: AtomicU64::new(0),
            session: Mutex::new(Session::default()),
        }
    }

    /// Remembers the protocol version the server chose, which later requests name.
    pub(super) fn negotiated(&self, version: &str) {
        let mut session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        session.version = Some(version.to_owned());
    }

    async fn post(&self, body: &Value) -> Result<reqwest::Response, McpError> {
        let mut request = self
            .client
            .post(self.url.clone())
            .header(header::ACCEPT, "application/json, text/event-stream")
            .json(body);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token.expose_secret());
        }
        {
            let session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(id) = &session.id {
                request = request.header("Mcp-Session-Id", id);
            }
            if let Some(version) = &session.version {
                request = request.header("MCP-Protocol-Version", version);
            }
        }
        let response = request.send().await?;
        if let Some(id) = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
        {
            let mut session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            session.id = Some(id.to_owned());
        }
        let status = response.status();
        if !status.is_success() {
            return Err(McpError::Http(format!("the server answered {status}")));
        }
        Ok(response)
    }

    pub(super) async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.last_id.fetch_add(1, Ordering::Relaxed) + 1;
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let response = self.post(&body).await?;
        let events = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        let wanted = json!(id);
        let mut stream = response.bytes_stream();
        let mut read = Vec::new();
        // Parsed from the first answer that names this request.
        let mut parsed = 0;
        while let Some(chunk) = stream.next().await {
            read.extend_from_slice(&chunk?);
            if read.len() > MAX_BODY {
                return Err(McpError::TooLong);
            }
            if events {
                for message in events_in(&read, &mut parsed) {
                    if message.get("id") == Some(&wanted) && message.get("method").is_none() {
                        return super::answer(message);
                    }
                }
            }
        }
        if events {
            return Err(McpError::Closed);
        }
        let message: Value = serde_json::from_slice(&read)
            .map_err(|e| McpError::Protocol(format!("the answer is not JSON: {e}")))?;
        let found = match message {
            Value::Array(batch) => batch.into_iter().find(|m| m.get("id") == Some(&wanted)),
            one => Some(one),
        };
        found.map_or_else(|| Err(McpError::Closed), super::answer)
    }

    pub(super) async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        let body = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.post(&body).await?;
        Ok(())
    }
}

/// The JSON messages in the events of `read` that end with a blank line, from byte `parsed` on,
/// which this moves past them.
fn events_in(read: &[u8], parsed: &mut usize) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some((len, gap)) = event_end(&read[*parsed..]) {
        let event = String::from_utf8_lossy(&read[*parsed..*parsed + len]).replace("\r\n", "\n");
        *parsed += len + gap;
        let data: Vec<&str> = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|data| data.strip_prefix(' ').unwrap_or(data))
            .collect();
        if let Ok(message) = serde_json::from_str::<Value>(&data.join("\n")) {
            out.push(message);
        }
    }
    out
}

/// Where the first event of `bytes` ends: its length and the length of the blank line after it.
fn event_end(bytes: &[u8]) -> Option<(usize, usize)> {
    [&b"\n\n"[..], &b"\r\n\r\n"[..]]
        .into_iter()
        .filter_map(|gap| {
            let at = bytes.windows(gap.len()).position(|w| w == gap)?;
            Some((at, gap.len()))
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_parsed_once_whole() {
        let mut parsed = 0;
        let first = b"event: message\ndata: {\"id\":1,\"result\":{}}\n\ndata: {\"id\"";
        let got = events_in(first, &mut parsed);
        assert_eq!(got.len(), 1);
        let mut more = first.to_vec();
        more.extend_from_slice(b":2}\n\n");
        let got = events_in(&more, &mut parsed);
        assert_eq!(got, [json!({"id": 2})]);
    }
}
