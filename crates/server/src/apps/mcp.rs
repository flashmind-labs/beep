//! Remote MCP servers each user connects with OAuth, starting with Fastmail.
//!
//! The phone only catches the OAuth redirect on a localhost port and hands the
//! code to the server. Tokens live in `mcp_servers` and never reach the phone.
//! Each user gets one long-lived `McpRegistry`, and a keep-alive task
//! reconnects every few hours so access tokens refresh and rotated refresh
//! tokens are saved before they lapse.

use crate::user::User;
use anyhow::{Result, bail};
use async_trait::async_trait;
use flashmind::{
    tools::mcp::{McpAuthRequired, McpConfigProvider, McpRegistry, McpServerConfig},
    types::tool::{Tool, ToolContext, ToolResult},
};
use reqwest::Url;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, PoisonError, Weak},
    time::{Duration, Instant},
};

pub const FASTMAIL: &str = "fastmail";

pub(crate) const KEEP_ALIVE: Duration = Duration::from_secs(6 * 60 * 60);

/// The fixed part of a known server's config. Stored rows only add cached
/// tools and credentials, so a row can never point the server at another URL.
pub(crate) fn preset(name: &str) -> Option<McpServerConfig> {
    match name {
        FASTMAIL => Some(McpServerConfig {
            name: FASTMAIL.to_owned(),
            url: Some("https://api.fastmail.com/mcp".to_owned()),
            ..Default::default()
        }),
        _ => None,
    }
}

/// Accepts only `http://localhost:<port>/...` style loopback redirects, the
/// only kind Fastmail allows and the only kind the phone listens on.
pub fn check_redirect(redirect_uri: &str) -> Result<()> {
    let url = Url::parse(redirect_uri)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "http" || !loopback || url.port().is_none() {
        bail!("redirect must be http on a loopback address with a port");
    }
    Ok(())
}

/// Reads and writes one user's rows in `mcp_servers`. Weak, since the
/// user owns the registry that owns this.
pub(crate) struct StoreProvider {
    pub(crate) user: Weak<User>,
}

impl StoreProvider {
    fn user(&self) -> Result<Arc<User>> {
        self.user
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("user is gone"))
    }
}

#[async_trait::async_trait]
impl McpConfigProvider for StoreProvider {
    /// Servers the user finished connecting. Half-started flows are left out.
    async fn list_configs(&self) -> Result<Vec<McpServerConfig>> {
        let mut configs = Vec::new();
        for row in self.user()?.mcp_servers().await? {
            let (Some(mut config), Some(credentials)) = (preset(&row.name), row.credentials) else {
                continue;
            };
            if let Ok(stored) = serde_json::from_value::<McpServerConfig>(row.config) {
                config.cached_tools = stored.cached_tools;
            }
            config.credentials = Some(credentials);
            configs.push(config);
        }
        Ok(configs)
    }

    /// Flashmind calls this with in-memory copies whose credentials may be
    /// stale, so they are dropped here and only `save_credentials` writes them.
    async fn save_config(&self, config: &McpServerConfig) -> Result<()> {
        if preset(&config.name).is_none() {
            bail!("unknown MCP server");
        }
        let mut config = config.clone();
        config.credentials = None;
        config.bearer_token = None;
        let value = serde_json::to_value(&config)?;
        self.user()?.save_mcp_config(&config.name, value).await
    }

    async fn delete_config(&self, name: &str) -> Result<()> {
        self.user()?.delete_mcp_server(name).await?;
        Ok(())
    }

    async fn save_credentials(&self, name: &str, credentials: &Value) -> Result<()> {
        self.user()?
            .save_mcp_credentials(name, credentials.clone())
            .await
    }

    async fn load_credentials(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .user()?
            .mcp_servers()
            .await?
            .into_iter()
            .find(|row| row.name == name)
            .and_then(|row| row.credentials))
    }
}

/// Display name for the notice when a server's login is lost.
pub(crate) fn display_name(name: &str) -> &str {
    match name {
        FASTMAIL => "Fastmail",
        _ => name,
    }
}

/// True when `error` means the server rejected the saved login, so only
/// connecting again from the phone can fix it.
pub(crate) fn lost_login(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<McpAuthRequired>())
}

/// When a tool call needs the user's OK first.
#[derive(Clone, Copy)]
enum Confirm {
    Never,
    Always,
    /// Only when it moves mail to another folder, which can bin it.
    WhenMoving,
}

impl Confirm {
    fn needed(self, args: &Value) -> bool {
        match self {
            Confirm::Never => false,
            Confirm::Always => true,
            Confirm::WhenMoving => args.get("folder").is_some_and(|folder| !folder.is_null()),
        }
    }

    /// Added to the tool's description.
    fn note(self) -> &'static str {
        match self {
            Confirm::Never => "",
            Confirm::Always => {
                " Needs the user's OK: the first call does nothing and asks you to check \
                 with them. Repeat the exact same call only after they agree."
            }
            Confirm::WhenMoving => {
                " Moving to another folder needs the user's OK: the first such call does \
                 nothing and asks you to check with them. Repeat the exact same call only \
                 after they agree."
            }
        }
    }
}

/// The Fastmail tools the agent gets, each as its own tool with a flat schema.
/// Weaker models leave out the nested `params` a generic `mcp` tool needs. The
/// rest are left out: the event and note deletes and `compose_event` only stage
/// a widget in Fastmail's own UI, and the prompt stays smaller.
const FASTMAIL_TOOLS: &[(&str, Confirm)] = &[
    ("search_email", Confirm::Never),
    ("read_email", Confirm::Never),
    ("read_thread", Confirm::Never),
    ("draft_email", Confirm::Never),
    ("update_email", Confirm::WhenMoving),
    ("archive_email", Confirm::Always),
    ("delete_email", Confirm::Always),
    ("search_events", Confirm::Never),
    ("create_event", Confirm::Never),
    ("update_event", Confirm::Never),
    ("rsvp_event", Confirm::Never),
    ("search_contacts", Confirm::Never),
    ("delete_contact", Confirm::Always),
];

/// How long a request for the user's OK stays open.
const CONFIRM_FOR: Duration = Duration::from_secs(15 * 60);

/// Calls waiting for the user's OK, by session, tool and arguments.
#[derive(Default)]
pub(crate) struct Confirmations(Mutex<HashMap<String, Asked>>);

struct Asked {
    run_id: String,
    at: Instant,
}

impl Confirmations {
    /// True when the same call was asked about in an earlier run of the
    /// session, so the user has had a turn to answer. Otherwise records the
    /// ask and returns false.
    fn granted(&self, key: String, run_id: &str) -> bool {
        let mut asked = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        asked.retain(|_, ask| ask.at.elapsed() < CONFIRM_FOR);
        if asked.get(&key).is_some_and(|ask| ask.run_id != run_id) {
            asked.remove(&key);
            return true;
        }
        asked.entry(key).or_insert_with(|| Asked {
            run_id: run_id.to_owned(),
            at: Instant::now(),
        });
        false
    }
}

/// The run a tool is called from, for the confirmations.
pub(crate) struct RunScope {
    pub(crate) run_id: String,
    pub(crate) session_id: String,
    pub(crate) confirmations: Arc<Confirmations>,
}

/// One remote tool, named like Composio's (`FASTMAIL_READ_EMAIL`) so the
/// phone labels it "Fastmail: read email".
pub(crate) struct AppTool {
    registry: McpRegistry,
    server: &'static str,
    tool: &'static str,
    name: String,
    description: String,
    schema: Value,
    confirm: Confirm,
    scope: Arc<RunScope>,
}

/// The agent tools for the servers in `connected`.
pub(crate) fn app_tools(
    registry: &McpRegistry,
    connected: &HashSet<String>,
    scope: &Arc<RunScope>,
) -> Vec<Arc<dyn Tool>> {
    let defs = registry.current_mcp_tools();
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    for server in connected {
        let (server, exposed) = match server.as_str() {
            FASTMAIL => (FASTMAIL, FASTMAIL_TOOLS),
            _ => continue,
        };
        let Some(defs) = defs.get(server) else {
            continue;
        };
        for &(name, confirm) in exposed {
            let Some(def) = defs.iter().find(|def| def.name == name) else {
                continue;
            };
            let description = def.description.clone().unwrap_or_default() + confirm.note();
            tools.push(Arc::new(AppTool {
                registry: registry.clone(),
                server,
                tool: name,
                name: format!("{server}_{name}").to_uppercase(),
                description,
                schema: def.input_schema.clone(),
                confirm,
                scope: scope.clone(),
            }));
        }
    }
    tools
}

#[async_trait]
impl Tool for AppTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args = if ctx.args.is_null() {
            json!({})
        } else {
            ctx.args
        };
        if self.confirm.needed(&args) {
            let key = format!("{}\0{}\0{args}", self.scope.session_id, self.name);
            if !self.scope.confirmations.granted(key, &self.scope.run_id) {
                return Ok(ToolResult::success(
                    ctx.tool_call_id,
                    "Not done yet: this needs the user's OK. Tell them exactly what will \
                     change (which emails or contact) and ask. If they agree, make this \
                     same call again with the same arguments. If not, leave it.",
                ));
            }
        }
        match self.registry.call_tool(self.server, self.tool, args).await {
            Ok(result) => {
                let text = result
                    .content
                    .iter()
                    .filter_map(|content| content.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(if result.is_error {
                    ToolResult::failure(ctx.tool_call_id, text)
                } else {
                    ToolResult::success(ctx.tool_call_id, text)
                })
            }
            Err(error) => {
                log::warn!("mcp: {}/{} failed: {error:#}", self.server, self.tool);
                Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("{} is not reachable right now.", display_name(self.server)),
                ))
            }
        }
    }

    fn max_output_bytes(&self) -> usize {
        usize::MAX
    }

    fn max_output_lines(&self) -> usize {
        usize::MAX
    }

    fn humanize(&self, _args: &Value) -> String {
        format!(
            "{}: {}",
            display_name(self.server),
            self.tool.replace('_', " ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;
    use serde_json::json;

    #[test]
    fn a_confirmed_call_runs_only_in_a_later_run() {
        let confirmations = Confirmations::default();
        assert!(!confirmations.granted("s\0del\0{}".into(), "run1"));
        assert!(!confirmations.granted("s\0del\0{}".into(), "run1"));
        assert!(confirmations.granted("s\0del\0{}".into(), "run2"));
        // Granted once: the next call asks again.
        assert!(!confirmations.granted("s\0del\0{}".into(), "run3"));
        assert!(!confirmations.granted("s\0del\0{\"id\":1}".into(), "run4"));
    }

    #[test]
    fn moving_mail_needs_confirming() {
        let update = Confirm::WhenMoving;
        assert!(!update.needed(&json!({"ids": ["M1"], "isRead": true})));
        assert!(update.needed(&json!({"ids": ["M1"], "folder": "Trash"})));
    }

    #[test]
    fn redirect_must_be_loopback_http_with_port() {
        assert!(check_redirect("http://localhost:43123/callback").is_ok());
        assert!(check_redirect("http://127.0.0.1:43123/callback").is_ok());
        assert!(check_redirect("http://[::1]:43123/callback").is_ok());
        assert!(check_redirect("http://localhost/callback").is_err());
        assert!(check_redirect("https://localhost:443/callback").is_err());
        assert!(check_redirect("http://evil.example:8080/callback").is_err());
        assert!(check_redirect("http://localhost.evil.example:8080/").is_err());
    }

    #[tokio::test]
    async fn provider_keeps_credentials_out_of_config() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = User::for_test(store, "alice");
        let provider = StoreProvider {
            user: Arc::downgrade(&user),
        };

        let mut config = preset(FASTMAIL).unwrap();
        provider.save_config(&config).await.unwrap();
        assert!(provider.list_configs().await.unwrap().is_empty());

        let credentials = serde_json::json!({"client_id": "c", "token_response": null});
        provider
            .save_credentials(FASTMAIL, &credentials)
            .await
            .unwrap();
        // A stale copy without credentials must not clear the saved ones.
        config.url = Some("https://evil.example/mcp".to_owned());
        provider.save_config(&config).await.unwrap();

        let configs = provider.list_configs().await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].credentials, Some(credentials));
        assert_eq!(
            configs[0].url.as_deref(),
            Some("https://api.fastmail.com/mcp")
        );

        provider
            .save_credentials(FASTMAIL, &Value::Null)
            .await
            .unwrap();
        assert!(provider.list_configs().await.unwrap().is_empty());
        assert!(provider.load_credentials(FASTMAIL).await.unwrap().is_none());
    }
}
