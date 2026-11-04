//! Remote MCP servers each user connects with OAuth, starting with Fastmail.
//! Tokens live in `mcp_servers` and never reach the phone.

use crate::{AsyncMutex, account::user::User, apps::permissions::Acts};
use anyhow::{Result, bail};
use async_trait::async_trait;
use flashmind::{
    tools::mcp::{
        McpAuthRequired, McpConfigProvider, McpContent, McpRegistry, McpServerConfig,
        McpToolCallResult,
    },
    types::tool::{Tool, ToolContext, ToolResult},
};
use reqwest::Url;
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::timeout;

pub const FASTMAIL: &str = "fastmail";

/// Most Fastmail accounts one user may connect.
pub(crate) const MAX_FASTMAIL_ACCOUNTS: usize = 5;

/// The argument that picks the Fastmail account when there are several.
const ACCOUNT: &str = "account";

/// Whether `name` is the server of a Fastmail account.
pub(crate) fn is_fastmail(name: &str) -> bool {
    name == FASTMAIL
        || name
            .strip_prefix("fastmail-")
            .and_then(|n| n.parse::<usize>().ok())
            .is_some_and(|n| (2..=MAX_FASTMAIL_ACCOUNTS).contains(&n))
}

/// The server name of the `n`th Fastmail account slot, counting from 1.
pub(crate) fn fastmail_name(n: usize) -> String {
    if n <= 1 {
        FASTMAIL.to_owned()
    } else {
        format!("{FASTMAIL}-{n}")
    }
}

/// One connected Fastmail account.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FastmailAccount {
    /// Its server name, such as `fastmail-2`.
    pub(crate) name: String,
    pub(crate) address: Option<String>,
}

impl FastmailAccount {
    /// Its address, or its server name until the address is known.
    fn label(&self) -> &str {
        self.address.as_deref().unwrap_or(&self.name)
    }
}

/// The account `wanted` names by address or server name, else the first.
pub(crate) fn pick_account<'a>(
    accounts: &'a [FastmailAccount],
    wanted: Option<&str>,
) -> Result<&'a FastmailAccount, String> {
    let wanted = wanted.map(str::trim).filter(|wanted| !wanted.is_empty());
    let found = match wanted {
        None => accounts.first(),
        Some(wanted) => accounts.iter().find(|account| {
            account.name.eq_ignore_ascii_case(wanted)
                || account
                    .address
                    .as_deref()
                    .is_some_and(|address| address.eq_ignore_ascii_case(wanted))
        }),
    };
    found.ok_or_else(|| {
        if accounts.is_empty() {
            return "Fastmail is not connected.".to_owned();
        }
        let known: Vec<&str> = accounts.iter().map(FastmailAccount::label).collect();
        format!(
            "No Fastmail account {}. Use one of: {}.",
            wanted.unwrap_or_default(),
            known.join(", ")
        )
    })
}

/// The first `email` in a `list_identities` result.
pub(crate) fn identity_address(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => map
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("email"))
            .and_then(|(_, value)| value.as_str())
            .filter(|address| address.contains('@'))
            .map(|address| address.trim().to_lowercase())
            .or_else(|| map.values().find_map(identity_address)),
        Value::Array(items) => items.iter().find_map(identity_address),
        _ => None,
    }
}

pub(crate) const KEEP_ALIVE: Duration = Duration::from_secs(6 * 60 * 60);

/// Timeout for one Fastmail request, under flashmind's 30 second connect timeout.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// A request to a Fastmail server that ran out of time.
#[derive(Debug, thiserror::Error)]
#[error("{server} did not answer within {} seconds", CALL_TIMEOUT.as_secs())]
pub(crate) struct NoAnswer {
    server: String,
}

/// One user's registry of Fastmail servers.
/// flashmind holds one lock over all connections during a call, so a stuck request discards the registry.
#[derive(Default)]
pub(crate) struct McpSlot {
    current: AsyncMutex<Option<Arc<McpRegistry>>>,
    /// Where the servers are when not at Fastmail: tests point it at a fake.
    url: OnceLock<String>,
    /// Whether account addresses are being read in the background.
    learning: AtomicBool,
}

impl McpSlot {
    /// The registry, made on first use from the servers `user` saved.
    pub(crate) async fn get(&self, user: &Weak<User>) -> Arc<McpRegistry> {
        let mut current = self.current.lock().await;
        if let Some(registry) = current.as_ref() {
            return registry.clone();
        }
        let provider = Arc::new(StoreProvider {
            user: user.clone(),
            url: self.url.get().cloned(),
        });
        let registry = Arc::new(McpRegistry::new(provider, None));
        registry.load_saved().await;
        *current = Some(registry.clone());
        registry
    }

    /// Forgets `stale` unless a newer registry already replaced it.
    pub(crate) async fn discard(&self, stale: &Arc<McpRegistry>) {
        let mut current = self.current.lock().await;
        if current
            .as_ref()
            .is_some_and(|registry| Arc::ptr_eq(registry, stale))
        {
            *current = None;
        }
    }

    /// Runs `request` with `CALL_TIMEOUT`, discarding the registry on timeout.
    pub(crate) async fn bounded<T>(
        &self,
        registry: &Arc<McpRegistry>,
        server: &str,
        request: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        match timeout(CALL_TIMEOUT, request).await {
            Ok(result) => result,
            Err(_) => {
                log::warn!("mcp: {server} did not answer in time, starting a new registry");
                self.discard(registry).await;
                Err(NoAnswer {
                    server: server.to_owned(),
                }
                .into())
            }
        }
    }

    /// Calls `tool` on `server`, giving up after `CALL_TIMEOUT`.
    pub(crate) async fn call(
        &self,
        registry: &Arc<McpRegistry>,
        server: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult> {
        let result = self
            .bounded(
                registry,
                server,
                registry.call_tool(server, tool, arguments),
            )
            .await;
        // The tools are made from `current_mcp_tools` on every run, so the
        // queued register and unregister ops are not needed.
        registry.drain_pending_ops();
        result
    }

    /// Marks a background read of addresses as started; false when one runs.
    pub(crate) fn start_learning(&self) -> bool {
        !self.learning.swap(true, Ordering::AcqRel)
    }

    pub(crate) fn done_learning(&self) {
        self.learning.store(false, Ordering::Release);
    }

    /// Sends every server's requests to `url` instead of to Fastmail.
    #[cfg(test)]
    pub(crate) fn point_at(&self, url: &str) {
        if self.url.set(url.to_owned()).is_err() {
            log::warn!("mcp: the test server was already set");
        }
    }
}

/// The fixed part of a known server's config, so a row can never change its URL.
pub(crate) fn preset(name: &str) -> Option<McpServerConfig> {
    is_fastmail(name).then(|| McpServerConfig {
        name: name.to_owned(),
        url: Some("https://api.fastmail.com/mcp".to_owned()),
        ..Default::default()
    })
}

/// Accepts only `http://localhost:<port>/...` loopback redirects.
pub fn check_redirect(redirect_uri: &str) -> Result<()> {
    let url = Url::parse(redirect_uri)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "http" || !loopback || url.port().is_none() {
        bail!("redirect must be http on a loopback address with a port");
    }
    Ok(())
}

/// Reads and writes one user's `mcp_servers` rows. Weak, since the user owns the registry.
pub(crate) struct StoreProvider {
    pub(crate) user: Weak<User>,
    /// Replaces the preset's URL; only tests set it.
    pub(crate) url: Option<String>,
}

impl StoreProvider {
    fn user(&self) -> Result<Arc<User>> {
        self.user
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("user is gone"))
    }
}

#[async_trait]
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
            if let Some(url) = &self.url {
                config.url = Some(url.clone());
            }
            configs.push(config);
        }
        Ok(configs)
    }

    /// Credentials here may be stale, so only `save_credentials` writes them.
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
    if is_fastmail(name) { "Fastmail" } else { name }
}

/// True when the server rejected the saved login.
pub(crate) fn lost_login(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<McpAuthRequired>())
}

/// The Fastmail tools the agent gets, each with a flat schema, and whether each acts.
pub(crate) const FASTMAIL_TOOLS: &[(&str, Acts)] = &[
    ("search_email", Acts::Never),
    ("read_email", Acts::Never),
    ("read_thread", Acts::Never),
    ("read_attachment", Acts::Never),
    ("list_folders", Acts::Never),
    ("draft_email", Acts::Never),
    ("upload_attachment", Acts::Never),
    ("send_email", Acts::Always),
    ("list_identities", Acts::Never),
    ("update_email", Acts::WhenMoving),
    ("archive_email", Acts::Always),
    ("delete_email", Acts::Always),
    ("list_calendars", Acts::Never),
    ("search_events", Acts::Never),
    ("create_event", Acts::Always),
    ("update_event", Acts::Always),
    ("delete_event", Acts::Always),
    ("rsvp_event", Acts::Always),
    ("search_contacts", Acts::Never),
    ("create_contact", Acts::Always),
    ("update_contact", Acts::Always),
    ("delete_contact", Acts::Always),
];

/// One remote tool named like Composio's (`FASTMAIL_READ_EMAIL`), with `account` when there are several.
pub(crate) struct AppTool {
    slot: Arc<McpSlot>,
    registry: Arc<McpRegistry>,
    accounts: Vec<FastmailAccount>,
    tool: &'static str,
    name: String,
    description: String,
    schema: Value,
}

/// The agent tools for the Fastmail `accounts`, the first being the default.
pub(crate) fn app_tools(
    slot: &Arc<McpSlot>,
    registry: &Arc<McpRegistry>,
    accounts: &[FastmailAccount],
) -> Vec<Arc<dyn Tool>> {
    let defs = registry.current_mcp_tools();
    let Some(defs) = accounts.iter().find_map(|account| defs.get(&account.name)) else {
        return Vec::new();
    };
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    for &(name, _) in FASTMAIL_TOOLS {
        let Some(def) = defs.iter().find(|def| def.name == name) else {
            continue;
        };
        let mut description = describe(name, def.description.as_deref().unwrap_or_default());
        let mut schema = def.input_schema.clone();
        if accounts.len() > 1 {
            add_account_arg(&mut schema, accounts);
            description.push_str(" Pass `account` to pick the Fastmail account; IDs from one account only work in that account.");
        }
        tools.push(Arc::new(AppTool {
            slot: slot.clone(),
            registry: registry.clone(),
            accounts: accounts.to_vec(),
            tool: name,
            name: format!("{FASTMAIL}_{name}").to_uppercase(),
            description,
            schema,
        }));
    }
    tools
}

/// Adds the `account` property to a tool's schema, listing the addresses.
fn add_account_arg(schema: &mut Value, accounts: &[FastmailAccount]) {
    let labels: Vec<&str> = accounts.iter().map(FastmailAccount::label).collect();
    let property = json!({
        "type": "string",
        "enum": labels,
        "description": format!(
            "Which Fastmail account to use. Defaults to {}.",
            labels.first().copied().unwrap_or_default()
        ),
    });
    if !schema.is_object() {
        *schema = json!({"type": "object"});
    }
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    let properties = object.entry("properties").or_insert_with(|| json!({}));
    if let Some(properties) = properties.as_object_mut() {
        properties.insert(ACCOUNT.to_owned(), property);
    }
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
        let mut args = drop_empty(ctx.args);
        if self.tool == "search_email" {
            fix_query(&mut args);
        }
        let wanted = take_account(&mut args);
        let account = match pick_account(&self.accounts, wanted.as_deref()) {
            Ok(account) => account,
            Err(message) => return Ok(ToolResult::failure(ctx.tool_call_id, message)),
        };
        // With one account there is nothing to tell apart.
        let from = if self.accounts.len() > 1 {
            format!("Account: {}\n", account.label())
        } else {
            String::new()
        };
        match self
            .slot
            .call(&self.registry, &account.name, self.tool, args)
            .await
        {
            Ok(mut result) => {
                if !from.is_empty() {
                    result
                        .content
                        .insert(0, McpContent::Text(from.trim_end().to_owned()));
                }
                // A file comes as an attachment, or fails the call if the model cannot take it.
                Ok(result.into_tool_result(ctx.tool_call_id))
            }
            Err(error) => {
                log::warn!("mcp: {}/{} failed: {error:#}", account.name, self.tool);
                Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("{from}Fastmail is not reachable right now."),
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

    fn humanize(&self, args: &Value) -> String {
        let tool = self.tool.replace('_', " ");
        if self.accounts.len() < 2 {
            return format!("Fastmail: {tool}");
        }
        let wanted = args.get(ACCOUNT).and_then(Value::as_str);
        match pick_account(&self.accounts, wanted) {
            Ok(account) => format!("Fastmail ({}): {tool}", account.label()),
            Err(_) => format!("Fastmail: {tool}"),
        }
    }
}

/// Removes and returns the `account` argument, which only Beep reads.
pub(crate) fn take_account(args: &mut Value) -> Option<String> {
    let taken = args.as_object_mut()?.remove(ACCOUNT)?;
    taken.as_str().map(str::to_owned)
}

/// What Fastmail says about `tool`, plus what the model still gets wrong.
fn describe(tool: &str, description: &str) -> String {
    match tool {
        "search_email" => format!(
            "{description} `from:` takes an address or the name of a saved contact; \
for a company or newsletter that is not a contact, search its name or domain as plain text."
        ),
        _ => description.to_owned(),
    }
}

/// Rewrites Gmail qualifiers Fastmail refuses (`newer_than:`, `older_than:`, `in:anywhere`).
fn fix_query(args: &mut Value) {
    let Some(Value::String(query)) = args.get_mut("query") else {
        return;
    };
    let fixed: Vec<String> = query
        .split_whitespace()
        .filter(|word| !matches!(word.to_lowercase().as_str(), "in:anywhere" | "in:all"))
        .map(|word| {
            let lower = word.to_lowercase();
            if let Some(age) = lower.strip_prefix("newer_than:") {
                format!("after:{age}")
            } else if let Some(age) = lower.strip_prefix("older_than:") {
                format!("before:{age}")
            } else {
                word.to_owned()
            }
        })
        .collect();
    *query = fixed.join(" ");
}

/// Drops empty arguments; Fastmail rejects a null or "None" `cursor`.
fn drop_empty(args: Value) -> Value {
    let Value::Object(mut args) = args else {
        return json!({});
    };
    args.retain(|_, value| match value {
        Value::Null => false,
        Value::String(text) => !matches!(text.trim(), "" | "None" | "null"),
        _ => true,
    });
    Value::Object(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;
    use serde_json::json;

    #[test]
    fn empty_arguments_are_left_out() {
        assert_eq!(
            drop_empty(json!({"query": "hotel", "cursor": null, "limit": 10})),
            json!({"query": "hotel", "limit": 10})
        );
        assert_eq!(
            drop_empty(json!({"query": "hotel", "cursor": "None"})),
            json!({"query": "hotel"})
        );
        assert_eq!(drop_empty(Value::Null), json!({}));
    }

    #[test]
    fn gmail_qualifiers_become_fastmail_ones() {
        let mut args =
            json!({"query": "from:bank newer_than:7d in:anywhere OLDER_THAN:1y", "limit": 5});
        fix_query(&mut args);
        assert_eq!(
            args,
            json!({"query": "from:bank after:7d before:1y", "limit": 5})
        );
        let mut args = json!({"limit": 5});
        fix_query(&mut args);
        assert_eq!(args, json!({"limit": 5}));
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
            url: None,
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
