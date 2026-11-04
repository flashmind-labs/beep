//! Remote MCP servers the user adds by URL.
//! Calls are rate limited per host and per user, and the URL must be https resolving only to public addresses.

use crate::{
    account::user::User,
    apps::mcp::lost_login,
    lock,
    protocol::{McpServer as McpServerInfo, McpTool, McpTools},
    store::{CustomMcp, McpServerRow},
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use flashmind::{
    tools::mcp::{McpConfigProvider, McpRegistry, McpServerConfig, McpToolDef},
    types::tool::{Tool, ToolContext, ToolResult},
};
use reqwest::Url;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, LazyLock, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::time::{Instant as TokioInstant, sleep_until};

/// Prefix of every added server's row name and tool name, so "gmail" cannot pose as Gmail.
pub(crate) const PREFIX: &str = "mcp_";
/// Longest part of a row name after `PREFIX`.
const MAX_SLUG: usize = 16;
/// Longest name a model provider takes for a tool.
const MAX_TOOL_NAME: usize = 64;
/// How many of a server's tools are on until the user picks.
pub(crate) const MAX_TOOLS: usize = 25;
/// Longest tool description and schema kept.
const MAX_DESCRIPTION: usize = 1000;
const MAX_SCHEMA_BYTES: usize = 8 * 1024;
/// Longest result a custom tool hands the model.
const MAX_OUTPUT_BYTES: usize = 32 * 1024;
/// Longest name and URL a user may give.
pub(crate) const MAX_LABEL: usize = 60;
pub(crate) const MAX_URL: usize = 2000;
/// Most servers one user may add.
pub(crate) const MAX_SERVERS: usize = 10;

/// Calls one host takes per second from all users together, and at once.
const HOST_CALLS_PER_SECOND: u32 = 5;
const HOST_IN_FLIGHT: u32 = 8;
/// Calls one user makes to one host per minute.
const USER_CALLS_PER_MINUTE: u32 = 20;
/// How long a host that said it is overloaded is left alone.
const PAUSE_ON_LIMIT: Duration = Duration::from_secs(60);
/// Failures in a row that make a host rest, and for how long.
const FAILURES_TO_REST: u32 = 5;
const REST: Duration = Duration::from_secs(5 * 60);
/// How long a host's addresses count as checked.
const RECHECK_AFTER: Duration = Duration::from_secs(5 * 60);
/// How long a connection stays open after its last call.
const IDLE: Duration = Duration::from_secs(5 * 60);

/// Limits shared by every user, since the limit is the host's.
pub(crate) static LIMITS: LazyLock<Limits> = LazyLock::new(Limits::default);

/// `mcp_` plus the lowercased label with non-alphanumerics as `-` (no `_`, so tool names split back).
pub(crate) fn server_name(label: &str) -> Option<String> {
    let mut slug = String::new();
    for c in label.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= MAX_SLUG {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    (!slug.is_empty()).then(|| format!("{PREFIX}{slug}"))
}

/// Whether `name` is a row name of a server the user added.
pub(crate) fn is_custom(name: &str) -> bool {
    name.starts_with(PREFIX)
}

/// Why a URL is refused, worded for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum UrlError {
    #[error("The address must be a full https:// URL.")]
    NotHttps,
    #[error("The address must not carry a user name or password.")]
    Credentials,
    #[error("Beep could not find that server.")]
    Unresolved,
    #[error("That address is on a private network, which Beep cannot reach.")]
    Private,
}

/// The URL when it is https without credentials and every address of its host is public.
pub(crate) async fn check_url(url: &str) -> Result<Url, UrlError> {
    let url = Url::parse(url.trim()).map_err(|_| UrlError::NotHttps)?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(UrlError::NotHttps);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlError::Credentials);
    }
    let host = url.host_str().ok_or(UrlError::NotHttps)?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| UrlError::Unresolved)?
        .map(|addr| addr.ip())
        .collect();
    if addrs.is_empty() {
        return Err(UrlError::Unresolved);
    }
    if !addrs.into_iter().all(is_public) {
        return Err(UrlError::Private);
    }
    Ok(url)
}

/// Whether `ip` is public; IPv6 must be global unicast (2000::/3), which excludes mesh ranges.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let first = ip.segments()[0];
            let global_unicast = first & 0xe000 == 0x2000;
            let documentation = first == 0x2001 && ip.segments()[1] == 0x0db8;
            // 6to4 and Teredo carry an IPv4 address the check above did not see.
            let tunnel = first == 0x2002 || (first == 0x2001 && ip.segments()[1] == 0);
            global_unicast && !documentation && !tunnel && ip != Ipv6Addr::UNSPECIFIED
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
        || a == 0
        || a >= 240
        // Shared address space (carrier-grade NAT).
        || (a == 100 && (64..128).contains(&b))
        // Benchmarking.
        || (a == 198 && (18..20).contains(&b)))
}

/// The host a call's limits are counted on.
pub(crate) fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default()
}

/// Why a call was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The host is resting after saying it was overloaded or failing.
    Paused,
    /// All users together reached the host's limit.
    HostBusy,
    /// This user reached their own limit on the host.
    UserLimit,
}

impl Refused {
    fn message(self, server: &str) -> String {
        match self {
            Refused::Paused => format!(
                "{server} is limiting requests right now. Tell the user to try again in a few minutes."
            ),
            Refused::HostBusy => format!("{server} is busy. Try again in a moment."),
            Refused::UserLimit => format!(
                "Too many calls to {server} in the last minute. Wait before calling it again, \
                 and use the results already in the chat."
            ),
        }
    }
}

/// How a call went, for the host's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Ok,
    /// The host answered 429 or 503, or said so in its result.
    Limited,
    Failed,
}

#[derive(Default)]
struct HostState {
    second: Option<Instant>,
    calls_this_second: u32,
    in_flight: u32,
    paused_until: Option<Instant>,
    failures: u32,
    checked_at: Option<Instant>,
}

#[derive(Default)]
struct UserWindow {
    minute: Option<Instant>,
    calls: u32,
}

/// Call limits per host and per user on a host.
#[derive(Default)]
pub(crate) struct Limits {
    hosts: Mutex<HashMap<String, HostState>>,
    users: Mutex<HashMap<(String, String), UserWindow>>,
}

/// A call under way; the host's in-flight count drops when it goes.
pub(crate) struct Permit<'a> {
    limits: &'a Limits,
    host: String,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut hosts) = self.limits.hosts.lock()
            && let Some(state) = hosts.get_mut(&self.host)
        {
            state.in_flight = state.in_flight.saturating_sub(1);
        }
    }
}

impl Permit<'_> {
    /// Records how the call went.
    pub(crate) fn finish(self, outcome: Outcome) {
        self.limits.record(&self.host, outcome, Instant::now());
    }
}

impl Limits {
    /// Takes a call slot for `user` on `host`, or says why not.
    pub(crate) fn begin(&self, host: &str, user: &str) -> Result<Permit<'_>, Refused> {
        self.begin_at(host, user, Instant::now())
    }

    fn begin_at(&self, host: &str, user: &str, now: Instant) -> Result<Permit<'_>, Refused> {
        let mut hosts = self.hosts.lock().map_err(|_| Refused::HostBusy)?;
        let state = hosts.entry(host.to_owned()).or_default();
        if state.paused_until.is_some_and(|until| now < until) {
            return Err(Refused::Paused);
        }
        if state
            .second
            .is_none_or(|start| now.duration_since(start) >= Duration::from_secs(1))
        {
            state.second = Some(now);
            state.calls_this_second = 0;
        }
        if state.calls_this_second >= HOST_CALLS_PER_SECOND || state.in_flight >= HOST_IN_FLIGHT {
            return Err(Refused::HostBusy);
        }
        let mut users = self.users.lock().map_err(|_| Refused::HostBusy)?;
        let window = users.entry((host.to_owned(), user.to_owned())).or_default();
        if window
            .minute
            .is_none_or(|start| now.duration_since(start) >= Duration::from_secs(60))
        {
            window.minute = Some(now);
            window.calls = 0;
        }
        if window.calls >= USER_CALLS_PER_MINUTE {
            return Err(Refused::UserLimit);
        }
        window.calls += 1;
        state.calls_this_second += 1;
        state.in_flight += 1;
        Ok(Permit {
            limits: self,
            host: host.to_owned(),
        })
    }

    fn record(&self, host: &str, outcome: Outcome, now: Instant) {
        let Ok(mut hosts) = self.hosts.lock() else {
            return;
        };
        let state = hosts.entry(host.to_owned()).or_default();
        match outcome {
            Outcome::Ok => state.failures = 0,
            Outcome::Limited => {
                log::warn!("mcp: {host} is limiting requests, pausing it");
                state.paused_until = Some(now + PAUSE_ON_LIMIT);
            }
            Outcome::Failed => {
                state.failures += 1;
                if state.failures >= FAILURES_TO_REST {
                    log::warn!(
                        "mcp: {host} failed {} times in a row, resting it",
                        state.failures
                    );
                    state.paused_until = Some(now + REST);
                    state.failures = 0;
                }
            }
        }
    }

    /// Whether `host` needs its addresses checked again.
    fn needs_check(&self, host: &str, now: Instant) -> bool {
        self.hosts.lock().map_or(true, |hosts| {
            hosts
                .get(host)
                .and_then(|state| state.checked_at)
                .is_none_or(|at| now.duration_since(at) >= RECHECK_AFTER)
        })
    }

    fn mark_checked(&self, host: &str, now: Instant) {
        if let Ok(mut hosts) = self.hosts.lock() {
            hosts.entry(host.to_owned()).or_default().checked_at = Some(now);
        }
    }
}

/// Checks `url` again when its host was not checked lately.
pub(crate) async fn recheck(url: &str) -> Result<(), UrlError> {
    let host = host_of(url);
    let now = Instant::now();
    if !LIMITS.needs_check(&host, now) {
        return Ok(());
    }
    check_url(url).await?;
    LIMITS.mark_checked(&host, now);
    Ok(())
}

/// How an error from a call reads for the host's state.
pub(crate) fn outcome_of(error: &anyhow::Error) -> Outcome {
    let text = format!("{error:#}").to_ascii_lowercase();
    if limited_text(&text) {
        Outcome::Limited
    } else {
        Outcome::Failed
    }
}

fn limited_text(text: &str) -> bool {
    text.contains("429")
        || text.contains("too many requests")
        || text.contains("rate limit")
        || text.contains("503")
        || text.contains("service unavailable")
}

/// What the app lists for an added server.
pub(crate) fn info(
    name: &str,
    custom: &CustomMcp,
    connected: bool,
    tools: Vec<String>,
) -> McpServerInfo {
    McpServerInfo {
        id: name.to_owned(),
        name: custom.label.clone(),
        url: custom.url.clone(),
        connected,
        sign_in: custom.sign_in,
        tools,
    }
}

/// The registry config for an added server: URL from the row, tools from its last connect.
pub(crate) fn config_of(row: &McpServerRow, custom: &CustomMcp) -> McpServerConfig {
    let cached_tools = serde_json::from_value::<McpServerConfig>(row.config.clone())
        .map(|stored| stored.cached_tools)
        .unwrap_or_default();
    McpServerConfig {
        name: row.name.clone(),
        url: Some(custom.url.clone()),
        cached_tools,
        ..Default::default()
    }
}

/// Whether the server needs no sign-in or the user signed in.
pub(crate) fn ready(row: &McpServerRow) -> bool {
    row.custom
        .as_ref()
        .is_some_and(|custom| !custom.sign_in || row.credentials.is_some())
}

/// Stores added servers' tools and tokens; lists none, so the registry connects each lazily.
pub(crate) struct CustomProvider {
    pub(crate) user: Weak<User>,
}

impl CustomProvider {
    fn user(&self) -> Result<Arc<User>> {
        self.user.upgrade().ok_or_else(|| anyhow!("user is gone"))
    }
}

#[async_trait]
impl McpConfigProvider for CustomProvider {
    async fn list_configs(&self) -> Result<Vec<McpServerConfig>> {
        Ok(Vec::new())
    }

    /// Keeps only the cached tools; the URL lives in its own column.
    async fn save_config(&self, config: &McpServerConfig) -> Result<()> {
        if !is_custom(&config.name) {
            anyhow::bail!("not an added MCP server");
        }
        let kept = McpServerConfig {
            name: config.name.clone(),
            cached_tools: config.cached_tools.clone(),
            ..Default::default()
        };
        self.user()?
            .save_mcp_config(&config.name, serde_json::to_value(&kept)?)
            .await
    }

    /// The row goes only when the user removes the server.
    async fn delete_config(&self, _name: &str) -> Result<()> {
        Ok(())
    }

    async fn save_credentials(&self, name: &str, credentials: &Value) -> Result<()> {
        self.user()?
            .save_mcp_credentials(name, credentials.clone())
            .await
    }

    async fn load_credentials(&self, name: &str) -> Result<Option<Value>> {
        self.user()?.mcp_credentials(name).await
    }
}

/// Registers `config` with `registry` unless it already is.
pub(crate) async fn register(registry: &McpRegistry, config: &McpServerConfig) -> Result<()> {
    if registry.get_config(&config.name).await.is_none() {
        registry.add(config.clone()).await?;
    }
    Ok(())
}

/// The users whose registry has a reap waiting.
static REAPS: LazyLock<Reaps> = LazyLock::new(Reaps::default);

/// Last call time of each user with a reap waiting, so each user has at most one reap task.
#[derive(Default)]
struct Reaps {
    last_call: Mutex<HashMap<String, TokioInstant>>,
}

impl Reaps {
    /// Notes a call; true when no reap waits yet.
    fn called(&self, user: &str, now: TokioInstant) -> bool {
        lock(&self.last_call).insert(user.to_owned(), now).is_none()
    }

    /// When `user`'s reap is due, or `None` once it is due at `now`.
    fn due(&self, user: &str, now: TokioInstant) -> Option<TokioInstant> {
        let mut calls = lock(&self.last_call);
        let at = *calls.get(user)? + IDLE;
        if at <= now {
            calls.remove(user);
            return None;
        }
        Some(at)
    }
}

/// Closes `user`'s idle registry connections after `IDLE`, one waiting task per user.
fn reap_later(registry: &McpRegistry, user: &str) {
    if !REAPS.called(user, TokioInstant::now()) {
        return;
    }
    let registry = registry.clone();
    let user = user.to_owned();
    tokio::spawn(async move {
        while let Some(at) = REAPS.due(&user, TokioInstant::now()) {
            sleep_until(at).await;
        }
        registry.reap_idle(IDLE).await;
    });
}

/// The agent's name for `tool` of server `name`, such as `MCP_MY-NOTES_SEARCH_PAGES`.
pub(crate) fn tool_name(name: &str, tool: &str) -> String {
    let mut out = name.to_ascii_uppercase();
    out.push('_');
    for c in tool.chars() {
        if out.len() >= MAX_TOOL_NAME {
            break;
        }
        out.push(if c.is_ascii_alphanumeric() {
            c.to_ascii_uppercase()
        } else {
            '_'
        });
    }
    out
}

/// Whether each tool's schema fits and its agent name is not taken by an earlier one.
fn usable(server: &str, defs: &[McpToolDef]) -> Vec<bool> {
    let mut seen = HashSet::new();
    defs.iter()
        .map(|def| {
            let fits = serde_json::to_vec(&def.input_schema)
                .is_ok_and(|schema| schema.len() <= MAX_SCHEMA_BYTES);
            fits && seen.insert(tool_name(server, &def.name))
        })
        .collect()
}

/// The usable tools the user turned on, or the first `MAX_TOOLS` until they pick.
fn enabled(server: &str, defs: &[McpToolDef], chosen: Option<&[String]>) -> Vec<bool> {
    let mut left = MAX_TOOLS;
    usable(server, defs)
        .into_iter()
        .zip(defs)
        .map(|(usable, def)| {
            usable
                && match chosen {
                    Some(chosen) => chosen.contains(&def.name),
                    None if left > 0 => {
                        left -= 1;
                        true
                    }
                    None => false,
                }
        })
        .collect()
}

/// The names of the tools the agent gets from a server.
pub(crate) fn enabled_names(
    server: &str,
    defs: &[McpToolDef],
    chosen: Option<&[String]>,
) -> Vec<String> {
    defs.iter()
        .zip(enabled(server, defs, chosen))
        .filter(|(_, on)| *on)
        .map(|(def, _)| def.name.clone())
        .collect()
}

/// Every tool of a server, as the app lists them.
pub(crate) fn tool_list(server: &str, defs: &[McpToolDef], chosen: Option<&[String]>) -> McpTools {
    let tools = defs
        .iter()
        .zip(usable(server, defs))
        .zip(enabled(server, defs, chosen))
        .map(|((def, usable), enabled)| McpTool {
            name: def.name.clone(),
            description: description_of(def),
            enabled,
            usable,
        })
        .collect();
    McpTools {
        tools,
        limit: MAX_TOOLS as u32,
    }
}

/// The user's picks after turning `tool` on or off; picks of unlisted tools are kept.
pub(crate) fn choose(
    server: &str,
    defs: &[McpToolDef],
    chosen: Option<&[String]>,
    tool: &str,
    on: bool,
) -> Vec<String> {
    let mut picks = match chosen {
        Some(chosen) => chosen.to_vec(),
        None => enabled_names(server, defs, None),
    };
    picks.retain(|name| name != tool);
    if on {
        picks.push(tool.to_owned());
    }
    picks
}

fn description_of(def: &McpToolDef) -> String {
    def.description
        .as_deref()
        .unwrap_or_default()
        .chars()
        .take(MAX_DESCRIPTION)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The agent tools the user turned on, from the ready servers' cached schemas.
pub(crate) fn custom_tools(
    registry: &McpRegistry,
    owner: &Weak<User>,
    user: &str,
    rows: &[McpServerRow],
) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    for row in rows.iter().filter(|row| ready(row)) {
        let Some(custom) = &row.custom else {
            continue;
        };
        let config = config_of(row, custom);
        let host = host_of(&custom.url);
        let defs = &config.cached_tools;
        let on = enabled(&row.name, defs, custom.tools.as_deref());
        for (def, _) in defs.iter().zip(on).filter(|(_, on)| *on) {
            tools.push(Arc::new(CustomTool {
                registry: registry.clone(),
                owner: owner.clone(),
                config: config.clone(),
                label: custom.label.clone(),
                host: host.clone(),
                user: user.to_owned(),
                tool: def.name.clone(),
                name: tool_name(&row.name, &def.name),
                description: format!(
                    "{} (from {}, a server the user added)",
                    description_of(def),
                    custom.label
                ),
                schema: def.input_schema.clone(),
            }));
        }
    }
    tools
}

/// A tool of a server the user added.
struct CustomTool {
    registry: McpRegistry,
    /// The user, to forget a login the server rejected.
    owner: Weak<User>,
    /// The server's config, registered on the first call.
    config: McpServerConfig,
    label: String,
    host: String,
    user: String,
    /// The tool's name on its server.
    tool: String,
    /// The name the agent calls it by.
    name: String,
    description: String,
    schema: Value,
}

#[async_trait]
impl Tool for CustomTool {
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
        let server = self.config.name.as_str();
        let url = self.config.url.as_deref().unwrap_or_default();
        if let Err(error) = recheck(url).await {
            log::warn!("mcp: {server} refused on recheck: {error}");
            return Ok(ToolResult::failure(ctx.tool_call_id, error.to_string()));
        }
        let permit = match LIMITS.begin(&self.host, &self.user) {
            Ok(permit) => permit,
            Err(refused) => {
                log::info!(
                    "mcp: call refused; server={server} host={} reason={refused:?}",
                    self.host
                );
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    refused.message(&self.label),
                ));
            }
        };
        register(&self.registry, &self.config).await?;
        let args = match ctx.args {
            Value::Object(_) => ctx.args.clone(),
            _ => json!({}),
        };
        let result = self.registry.call_tool(server, &self.tool, args).await;
        reap_later(&self.registry, &self.user);
        match result {
            Ok(result) => {
                let limited = result.is_error
                    && result
                        .content
                        .iter()
                        .filter_map(|content| content.as_text())
                        .any(|text| limited_text(&text.to_ascii_lowercase()));
                permit.finish(if limited {
                    Outcome::Limited
                } else {
                    Outcome::Ok
                });
                // A file comes as an attachment, or fails the call if the model cannot take it.
                Ok(result.into_tool_result(ctx.tool_call_id))
            }
            Err(error) => {
                log::warn!("mcp: {server}/{} failed: {error:#}", self.tool);
                permit.finish(outcome_of(&error));
                if lost_login(&error)
                    && let Some(owner) = self.owner.upgrade()
                {
                    owner.drop_lost_mcp_login(server).await;
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        format!(
                            "{} needs the user to sign in again under Apps in Beep's menu.",
                            self.label
                        ),
                    ));
                }
                Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("{} is not reachable right now.", self.label),
                ))
            }
        }
    }

    fn timeout_secs(&self) -> Option<u64> {
        Some(60)
    }

    fn max_output_bytes(&self) -> usize {
        MAX_OUTPUT_BYTES
    }

    fn concurrent(&self) -> bool {
        false
    }

    fn humanize(&self, _args: &Value) -> String {
        format!("{}: {}", self.label, self.tool.replace('_', " "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs(names: &[&str]) -> Vec<McpToolDef> {
        names
            .iter()
            .map(|name| McpToolDef {
                name: (*name).to_owned(),
                description: None,
                input_schema: json!({"type": "object"}),
            })
            .collect()
    }

    #[test]
    fn a_clashing_name_is_not_usable() {
        let defs = defs(&["get-item", "get_item", "other"]);
        let usable: Vec<bool> = tool_list("mcp_notes", &defs, None)
            .tools
            .iter()
            .map(|tool| tool.usable)
            .collect();
        assert_eq!(usable, [true, false, true]);
    }

    #[test]
    fn only_public_addresses_pass() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
            "200:1234::1",
            "2001:db8::1",
            "2002:7f00:1::1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip} passed");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip} refused");
        }
    }

    #[tokio::test]
    async fn urls_must_be_https_without_credentials() {
        assert_eq!(
            check_url("http://example.com/mcp").await,
            Err(UrlError::NotHttps)
        );
        assert_eq!(check_url("example.com/mcp").await, Err(UrlError::NotHttps));
        assert_eq!(
            check_url("https://user:pw@example.com/mcp").await,
            Err(UrlError::Credentials)
        );
        assert_eq!(
            check_url("https://127.0.0.1/mcp").await,
            Err(UrlError::Private)
        );
        assert_eq!(
            check_url("https://[::1]:8443/mcp").await,
            Err(UrlError::Private)
        );
        assert_eq!(
            check_url("https://localhost/mcp").await,
            Err(UrlError::Private)
        );
    }

    #[test]
    fn a_host_takes_a_few_calls_a_second_from_everyone() {
        let limits = Limits::default();
        let now = Instant::now();
        let mut permits = Vec::new();
        for i in 0..HOST_CALLS_PER_SECOND {
            permits.push(
                limits
                    .begin_at("mcp.example", &format!("user{i}"), now)
                    .unwrap(),
            );
        }
        assert_eq!(
            limits.begin_at("mcp.example", "other", now).err(),
            Some(Refused::HostBusy)
        );
        // Another host has its own budget.
        assert!(limits.begin_at("other.example", "other", now).is_ok());
        drop(permits);
        let later = now + Duration::from_secs(1);
        assert!(limits.begin_at("mcp.example", "other", later).is_ok());
    }

    #[test]
    fn one_user_gets_a_few_calls_a_minute() {
        let limits = Limits::default();
        let start = Instant::now();
        for i in 0..USER_CALLS_PER_MINUTE {
            // Spread out so the host's per-second budget is not what stops it.
            let at = start + Duration::from_secs(u64::from(i));
            limits.begin_at("mcp.example", "alice", at).unwrap();
        }
        let at = start + Duration::from_secs(30);
        assert_eq!(
            limits.begin_at("mcp.example", "alice", at).err(),
            Some(Refused::UserLimit)
        );
        assert!(limits.begin_at("mcp.example", "bob", at).is_ok());
        let next_minute = start + Duration::from_secs(61);
        assert!(limits.begin_at("mcp.example", "alice", next_minute).is_ok());
    }
}
