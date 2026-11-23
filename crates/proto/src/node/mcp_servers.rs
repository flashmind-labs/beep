//! User-scoped MCP entries from an agent harness's native configuration.

use crate::SecretText;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROTOCOL: &str = "mcp-servers/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::AsRefStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Harness {
    Codex,
    Claude,
    /// Antigravity, by its CLI's name.
    Agy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    /// Unsupported or malformed native entries can still be listed and removed.
    pub settings: Option<Settings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditableServer {
    pub name: String,
    pub settings: Settings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub connection: Connection,
    /// Codex and Antigravity can disable an entry in their user configuration.
    pub enabled: bool,
    #[serde(default)]
    pub codex: Option<CodexOptions>,
    #[serde(default)]
    pub claude_oauth: Option<ClaudeOauth>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodexOptions {
    pub required: Option<bool>,
    pub startup_timeout_sec: Option<f64>,
    pub tool_timeout_sec: Option<f64>,
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
    pub default_tools_approval_mode: Option<ApprovalMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    Auto,
    Prompt,
    Writes,
    Approve,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaudeOauth {
    pub client_id: Option<String>,
    pub callback_port: Option<u16>,
    pub auth_server_metadata_url: Option<SecretText>,
    pub scopes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Connection {
    Stdio(Stdio),
    Remote(Remote),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stdio {
    pub command: String,
    pub args: Vec<SecretText>,
    pub env: BTreeMap<String, SecretText>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub transport: RemoteTransport,
    pub url: SecretText,
    pub headers: BTreeMap<String, SecretText>,
    pub env_http_headers: BTreeMap<String, String>,
    pub bearer_token_env_var: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::AsRefStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum RemoteTransport {
    Http,
    Sse,
    Ws,
}

/// The complete selected entry, for copying between nodes of the same harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeServer {
    pub name: String,
    pub config: SecretText,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Harnesses,
    List {
        harness: Harness,
    },
    Save {
        harness: Harness,
        previous: Option<Box<EditableServer>>,
        server: Box<EditableServer>,
    },
    Remove {
        harness: Harness,
        name: String,
    },
    Export {
        harness: Harness,
        name: String,
    },
    Apply {
        harness: Harness,
        server: NativeServer,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    Harnesses(Vec<Harness>),
    Servers(Vec<Server>),
    Applied,
    Refused(String),
    NativeServer(NativeServer),
    Conflict(Vec<Server>),
}
