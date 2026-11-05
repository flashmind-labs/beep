//! One open connection to a server: the handshake, its tool list and calling a tool.

use super::{
    McpError,
    config::{McpServer, Transport},
    http::Http,
    stdio::Stdio,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::timeout;

/// The protocol version asked for. The server answers with the one it speaks.
const PROTOCOL: &str = "2025-06-18";
/// The whole start: process, handshake and tool list.
const OPEN_LIMIT: Duration = Duration::from_secs(30);
/// One tool call.
const CALL_LIMIT: Duration = Duration::from_secs(60);
/// Pages of the tool list read, and tools kept from them.
const MAX_PAGES: usize = 10;
const MAX_TOOLS: usize = 200;
/// Largest input schema passed on to the model, serialized.
const MAX_SCHEMA: usize = 16 * 1024;

enum Wire {
    Stdio(Stdio),
    Http(Http),
}

/// A tool as the server lists it.
pub(super) struct RawTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    /// The server marks it `readOnlyHint: true`.
    pub read_only: bool,
}

/// What a tool call came to.
pub(crate) struct Called {
    pub text: String,
    pub ok: bool,
}

pub(super) struct Conn {
    wire: Wire,
    pub(super) tools: Vec<RawTool>,
}

/// The result of a JSON-RPC response, or its error.
pub(super) fn answer(message: Value) -> Result<Value, McpError> {
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the server reported an error");
        return Err(McpError::Server(text.to_owned()));
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

impl Wire {
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        match self {
            Self::Stdio(stdio) => stdio.request(method, params).await,
            Self::Http(http) => http.request(method, params).await,
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        match self {
            Self::Stdio(stdio) => stdio.notify(method, params).await,
            Self::Http(http) => http.notify(method, params).await,
        }
    }
}

impl Conn {
    /// Starts the server, shakes hands and lists its tools.
    pub(super) async fn open(server: &McpServer) -> Result<Self, McpError> {
        let wire = match &server.transport {
            Transport::Stdio { command, args, env } => {
                Wire::Stdio(Stdio::start(command, args, env)?)
            }
            Transport::Http { url, token } => Wire::Http(Http::new(url.clone(), token.clone())),
        };
        // Dropped on a failure, which ends a process started above.
        let mut conn = Self {
            wire,
            tools: Vec::new(),
        };
        timeout(OPEN_LIMIT, conn.handshake())
            .await
            .map_err(|_| McpError::Timeout)??;
        Ok(conn)
    }

    async fn handshake(&mut self) -> Result<(), McpError> {
        let params = json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": {"name": "beep", "version": env!("CARGO_PKG_VERSION")},
        });
        let hello = self.wire.request("initialize", params).await?;
        if let (Wire::Http(http), Some(version)) = (
            &self.wire,
            hello.get("protocolVersion").and_then(Value::as_str),
        ) {
            http.negotiated(version);
        }
        self.wire
            .notify("notifications/initialized", json!({}))
            .await?;
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = cursor
                .as_ref()
                .map_or_else(|| json!({}), |c| json!({"cursor": c}));
            let page = self.wire.request("tools/list", params).await?;
            for tool in page
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if self.tools.len() < MAX_TOOLS
                    && let Some(tool) = raw_tool(tool)
                {
                    self.tools.push(tool);
                }
            }
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(())
    }

    /// Calls `tool`. A tool that ran and failed is a result with `ok` false; a server that could
    /// not be asked is an error.
    pub(super) async fn call(&self, tool: &str, arguments: Value) -> Result<Called, McpError> {
        let params = json!({"name": tool, "arguments": arguments});
        let result = timeout(CALL_LIMIT, self.wire.request("tools/call", params))
            .await
            .map_err(|_| McpError::Timeout)??;
        Ok(Called {
            text: result_text(&result),
            ok: result.get("isError").and_then(Value::as_bool) != Some(true),
        })
    }

    /// Ends the process of a command server now.
    pub(super) fn close(&self) {
        if let Wire::Stdio(stdio) = &self.wire {
            stdio.kill();
        }
    }
}

fn raw_tool(tool: &Value) -> Option<RawTool> {
    let name = tool.get("name")?.as_str()?.to_owned();
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let schema = tool
        .get("inputSchema")
        .filter(|s| s.is_object() && s.to_string().len() <= MAX_SCHEMA)
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    let read_only = tool
        .pointer("/annotations/readOnlyHint")
        .and_then(Value::as_bool)
        == Some(true);
    Some(RawTool {
        name,
        description,
        schema,
        read_only,
    })
}

/// The text of a call result: its text parts, with a note for what cannot be shown.
fn result_text(result: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
        let part = match kind {
            "text" => item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            "image" | "audio" => format!("[{kind} not shown]"),
            "resource" => item
                .pointer("/resource/text")
                .and_then(Value::as_str)
                .map_or_else(|| "[resource not shown]".to_owned(), str::to_owned),
            "resource_link" => item
                .get("uri")
                .and_then(Value::as_str)
                .map_or_else(|| "[link]".to_owned(), |uri| format!("[link: {uri}]")),
            _ => "[content not shown]".to_owned(),
        };
        parts.push(part);
    }
    if parts.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        return structured.to_string();
    }
    parts.join("\n")
}
