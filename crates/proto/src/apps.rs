//! Connected apps, their MCP servers and how freely Beep acts in them.

use crate::ApprovalField;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// One connected account of an app such as Fastmail.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppAccount {
    pub id: String,
    /// The account's email address, once the server has read it.
    pub address: Option<String>,
}

/// A remote MCP server the user added. The server makes every call, so it
/// works the same from every device.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub url: String,
    pub connected: bool,
    /// Whether it signs in with OAuth.
    pub sign_in: bool,
    /// The tools Beep gets from it, as the server names them.
    pub tools: Vec<String>,
}

/// The tools of an added server, in the order it lists them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTools {
    pub tools: Vec<McpTool>,
    /// How many tools of one server the agent works well with. Until the
    /// user picks, the first `limit` usable tools are on.
    pub limit: u32,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    /// The tool's name on its server.
    pub name: String,
    pub description: String,
    /// Whether the agent gets it.
    pub enabled: bool,
    /// False when Beep cannot use it, such as a schema too large to send
    /// the model. It stays off.
    pub usable: bool,
}

/// An added or reconnected server: connected, or waiting for the user to
/// sign in at `auth_url`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpConnect {
    pub server: McpServer,
    pub auth_url: Option<String>,
}

wire_enum! {
    /// How freely Beep acts in an app. Reading is always allowed; the level
    /// covers tools that send, change or delete.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum PermissionLevel {
        0 Ask {}
        1 Allow {}
        2 ReadOnly {}
    }
}

impl PermissionLevel {
    /// The name stored on the server.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask {} => "ask",
            Self::Allow {} => "allow",
            Self::ReadOnly {} => "read",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "ask" => Some(Self::Ask {}),
            "allow" => Some(Self::Allow {}),
            "read" => Some(Self::ReadOnly {}),
            _ => None,
        }
    }
}

/// One app's permissions.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppPermissions {
    pub level: PermissionLevel,
    /// The app's tools that act, in the order Beep lists them.
    pub actions: Vec<AppAction>,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppAction {
    pub tool: String,
    /// Such as "Send email".
    pub label: String,
    /// Runs without asking under `Ask`.
    pub allowed: bool,
}

wire_enum! {
    /// Who provides an app connection: `Composio` apps are connected per user
    /// through OAuth, `Fastmail` is set up on the server with an API token.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AppKind {
        0 Composio {}
        1 Fastmail {}
    }
}

/// An app the assistant can use on the user's behalf.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppIntegration {
    pub slug: String,
    pub name: String,
    /// One short line.
    pub description: String,
    pub logo_url: Option<String>,
    pub connected: bool,
    /// False when the server lacks the credentials this app needs, so it
    /// cannot be connected from the phone.
    pub configured: bool,
    pub kind: AppKind,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectLink {
    pub url: String,
}

/// A standing permission the user approved once for one routine: each run
/// of the routine may make up to `per_run` calls of one action without
/// asking, when the call matches what the user approved. Anything else
/// still asks.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    /// The `Cron::id` of the routine it belongs to.
    pub routine_id: String,
    /// The routine's label, or its prompt when it has none.
    pub routine: String,
    /// The app's name, such as "Gmail".
    pub app: String,
    /// What it allows, such as "reply to thread".
    pub action: String,
    /// What the user approved, labelled for reading, as the card showed it.
    pub fields: Vec<ApprovalField>,
    pub per_run: u32,
    pub expires_at: SystemTime,
    pub created_at: SystemTime,
    /// When a call last used it; `None` before the first.
    pub last_used_at: Option<SystemTime>,
}
