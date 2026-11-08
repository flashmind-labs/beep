//! What Beep may do in each connected app: reads always, acting tools by the app's level (ask, allow, read only).

mod actions;
mod guard;
mod records;
pub(crate) use actions::{actions_since, went_through};
pub(crate) use guard::{
    ApprovalPayload, DENIED, SKIPPED, SUPERSEDED, allow_always_in, allow_in_chat, close_open_calls,
    guard, open_calls, read_tool_app, read_tools_only, waiting_approval,
};
pub(crate) use records::{describe_call, field_label, is_id, read_back, shorten};
use records::{fields, parse_output, records};

use crate::{
    apps::{
        CATALOG, CatalogApp, catalog_app, custom_mcp::PREFIX as ADDED_PREFIX, mcp::FASTMAIL_TOOLS,
        read,
    },
    protocol::{AppAction, AppKind, AppPermissions, Approval, ApprovalField, PermissionLevel},
    store::{AppPermissionRow, ChatAllow},
    tools::media::{self, MediaKind},
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Most argument fields shown on an approval, and the longest value.
const MAX_FIELDS: usize = 8;
/// Most records an approval names, each with its own few fields.
const MAX_RECORDS: usize = 12;
const MAX_VALUE_CHARS: usize = 600;

/// Whether a tool call acts, rather than only reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acts {
    Never,
    Always,
    /// Only when it moves mail to another folder, which can bin it.
    WhenMoving,
}

impl Acts {
    fn on(self, args: &Value) -> bool {
        match self {
            Acts::Never => false,
            Acts::Always => true,
            Acts::WhenMoving => args.get("folder").is_some_and(|folder| !folder.is_null()),
        }
    }

    /// Whether some call of the tool can act, and so falls under the rules.
    fn can_act(self) -> bool {
        self != Acts::Never
    }
}

/// Composio tags of tools that create or destroy, which ask even when listed as reads.
const ACTING_HINTS: &[&str] = &["createHint", "destructiveHint"];

/// Whether a Composio tool's tags say it creates or destroys something.
pub(crate) fn hinted_to_act(tags: &[String]) -> bool {
    tags.iter().any(|tag| ACTING_HINTS.contains(&tag.as_str()))
}

/// The project agent's own tools, gated by `crate::projects::gate` rather than `guard`, hence `Acts::Never`.
/// `exec` and `process` are here so a reloaded project chat shows their card; [`lookup`] leaves them out.
pub(crate) const PROJECT_APP: CatalogApp = CatalogApp {
    slug: "project",
    name: "Project",
    description: "The project agent's tasks and machines",
    kind: AppKind::Composio {},
    tools: &[
        read("PROJECT_SETUP"),
        read("PROJECT_PLAN"),
        read("PROJECT_TASKS"),
        read("PROJECT_WORKER"),
        read("PROJECT_REPLY"),
        read("EXEC"),
        read("PROCESS"),
        read("PROJECT_SETUP_CHECKOUT"),
        read("PROJECT_TASKS_UPDATE"),
        read("PROJECT_TASKS_DROP"),
        read("PROJECT_TASKS_REOPEN"),
        read("PROCESS_STOP"),
    ],
};

/// Project actions the user can allow always; merges and plan cards always ask.
const PROJECT_ALWAYS: &[&str] = &[
    "EXEC",
    "PROCESS_STOP",
    "PROJECT_PLAN",
    "PROJECT_SETUP_CHECKOUT",
    "PROJECT_TASKS_DROP",
    "PROJECT_TASKS_REOPEN",
    "PROJECT_TASKS_UPDATE",
    "PROJECT_WORKER",
];

/// A tool's name as rules keep it: upper case, since Composio's wrappers lowercase slugs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ToolSlug(String);

impl ToolSlug {
    pub(crate) fn new(tool: &str) -> Self {
        Self(tool.to_ascii_uppercase())
    }
}

impl AsRef<str> for ToolSlug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The app a tool belongs to: a catalog app, or an added server by row name.
#[derive(Clone)]
pub(crate) enum ToolApp {
    Catalog(&'static CatalogApp),
    Added(String),
    /// Beep's media tools that spend the user's own API keys.
    Media,
}

/// The rules' name for media made with the user's keys.
const MEDIA_APP: &str = "media";

impl AsRef<str> for ToolApp {
    fn as_ref(&self) -> &str {
        match self {
            ToolApp::Catalog(app) => app.slug,
            ToolApp::Added(slug) => slug,
            ToolApp::Media => MEDIA_APP,
        }
    }
}

impl ToolApp {
    /// The app's name on an approval: "My notes" for `mcp_my-notes`.
    pub(crate) fn name(&self) -> String {
        match self {
            ToolApp::Catalog(app) => app.name.to_owned(),
            ToolApp::Added(slug) => {
                capitalize(&slug.trim_start_matches(ADDED_PREFIX).replace('-', " "))
            }
            ToolApp::Media => "Media generation".to_owned(),
        }
    }
}

/// The app a tool belongs to and whether it acts; every added server's tool acts.
pub(crate) fn lookup(tool: &str) -> Option<(ToolApp, Acts)> {
    let tool = ToolSlug::new(tool);
    let tool = tool.as_ref();
    if tool.parse::<MediaKind>().is_ok() {
        return Some((ToolApp::Media, Acts::Always));
    }
    for app in CATALOG {
        if let Some(picked) = app.tools.iter().find(|picked| picked.slug == tool) {
            return Some((ToolApp::Catalog(app), picked.acts));
        }
    }
    let added = ADDED_PREFIX.to_ascii_uppercase();
    if let Some(rest) = tool.strip_prefix(&added) {
        let (server, name) = rest.split_once('_')?;
        if server.is_empty() || name.is_empty() {
            return None;
        }
        let slug = format!("{ADDED_PREFIX}{}", server.to_ascii_lowercase());
        return Some((ToolApp::Added(slug), Acts::Always));
    }
    if let Some(name) = tool.strip_prefix("FASTMAIL_") {
        let name = name.to_lowercase();
        let &(_, acts) = FASTMAIL_TOOLS.iter().find(|(known, _)| *known == name)?;
        return Some((ToolApp::Catalog(catalog_app("fastmail")?), acts));
    }
    if PROJECT_ONLY.contains(&tool) {
        return None;
    }
    project_tool(tool)
}

/// Tools every chat has that only a project chat gates as Project's.
const PROJECT_ONLY: [&str; 2] = ["EXEC", "PROCESS"];

/// [`lookup`] in a chat that may be a project's.
pub(crate) fn lookup_in(tool: &str, is_project: bool) -> Option<(ToolApp, Acts)> {
    if is_project && let Some(found) = project_tool(ToolSlug::new(tool).as_ref()) {
        return Some(found);
    }
    lookup(tool)
}

fn project_tool(slug: &str) -> Option<(ToolApp, Acts)> {
    let picked = PROJECT_APP
        .tools
        .iter()
        .find(|picked| picked.slug == slug)?;
    Some((ToolApp::Catalog(&PROJECT_APP), picked.acts))
}

/// The catalog app of `tool` and whether a call with `args` acts, for calls outside a session.
pub(crate) fn call_acts(tool: &str, args: &Value) -> Option<(&'static CatalogApp, bool)> {
    match lookup(tool)? {
        (ToolApp::Catalog(app), acts) => Some((app, acts.on(args))),
        (ToolApp::Added(_) | ToolApp::Media, _) => None,
    }
}

/// The card that asks to allow `tool` with `args`.
pub(crate) fn approval_for(app: &ToolApp, tool: &str, args: &Value) -> Approval {
    Approval {
        app: app.name(),
        action: action_name(app, ToolSlug::new(tool).as_ref()),
        fields: call_fields(app, tool, args),
    }
}

/// The arguments a card shows; media calls always show service and model.
fn call_fields(app: &ToolApp, tool: &str, args: &Value) -> Vec<ApprovalField> {
    match (app, tool.parse::<MediaKind>().ok()) {
        (ToolApp::Media, Some(kind)) => media::approval_fields(kind, args),
        _ => fields(args),
    }
}

/// The tools of `app` that can act; every project tool counts.
fn action_tools(app: &CatalogApp) -> Vec<String> {
    if app.slug == PROJECT_APP.slug {
        return PROJECT_ALWAYS.iter().map(|&slug| slug.to_owned()).collect();
    }
    if app.slug == "fastmail" {
        return FASTMAIL_TOOLS
            .iter()
            .filter(|(_, acts)| acts.can_act())
            .map(|(name, _)| format!("FASTMAIL_{}", name.to_uppercase()))
            .collect();
    }
    app.tools
        .iter()
        .filter(|picked| picked.acts.can_act())
        .map(|picked| picked.slug.to_owned())
        .collect()
}

/// "send email" for `GMAIL_SEND_EMAIL`.
pub(crate) fn action_name(app: &ToolApp, tool: &str) -> String {
    if let (ToolApp::Media, Some(kind)) = (app, tool.parse::<MediaKind>().ok()) {
        return kind.action().to_owned();
    }
    let prefix = format!("{}_", app.as_ref().to_uppercase());
    tool.strip_prefix(&prefix)
        .unwrap_or(tool)
        .to_lowercase()
        .replace('_', " ")
}

pub(crate) fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// One app's level and the actions that run without asking.
#[derive(Debug, Clone)]
pub(crate) struct Rule {
    level: PermissionLevel,
    allowed: HashSet<String>,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            level: PermissionLevel::Ask {},
            allowed: HashSet::new(),
        }
    }
}

/// Every app's rule, by slug. An app with none asks first.
#[derive(Debug, Clone, Default)]
pub(crate) struct Rules(HashMap<String, Rule>);

impl From<Vec<AppPermissionRow>> for Rules {
    fn from(rows: Vec<AppPermissionRow>) -> Self {
        Self(
            rows.into_iter()
                .map(|row| {
                    let rule = Rule {
                        level: row.level,
                        allowed: row.allowed.into_iter().collect(),
                    };
                    (row.app, rule)
                })
                .collect(),
        )
    }
}

impl Rules {
    pub(crate) fn level(&self, app: &str) -> PermissionLevel {
        self.0
            .get(app)
            .map_or(Rule::default().level, |rule| rule.level)
    }

    /// Whether `tool` of `app` runs without asking under ask first.
    pub(crate) fn allows(&self, app: &str, tool: &str) -> bool {
        self.0
            .get(app)
            .is_some_and(|rule| rule.allowed.contains(tool))
    }

    /// Whether `app` may only read.
    pub(crate) fn read_only(&self, app: &str) -> bool {
        self.level(app) == PermissionLevel::ReadOnly {}
    }

    /// Adds what the user allowed for the rest of one chat; read only still refuses.
    pub(crate) fn allow_in_chat(&mut self, allows: Vec<ChatAllow>) {
        for ChatAllow { app, tool } in allows {
            self.0.entry(app).or_default().allowed.insert(tool);
        }
    }
}

/// What the phone shows for `app`.
pub(crate) fn app_permissions(app: &'static CatalogApp, rules: &Rules) -> AppPermissions {
    let actions = action_tools(app)
        .into_iter()
        .map(|tool| AppAction {
            label: capitalize(&action_name(&ToolApp::Catalog(app), &tool)),
            allowed: rules.allows(app.slug, &tool),
            tool,
        })
        .collect();
    AppPermissions {
        level: rules.level(app.slug),
        actions,
    }
}

/// The row to save for `app`, keeping only actions it has.
pub(crate) fn row(
    app: &CatalogApp,
    level: PermissionLevel,
    allowed: Vec<String>,
) -> AppPermissionRow {
    let known = action_tools(app);
    let mut allowed: Vec<String> = allowed
        .into_iter()
        .filter(|tool| known.contains(tool))
        .collect();
    allowed.sort();
    allowed.dedup();
    AppPermissionRow {
        app: app.slug.to_owned(),
        level,
        allowed,
    }
}

/// The tool call the user just allowed once, which runs without asking.
pub(crate) struct Approved(pub(crate) Option<String>);

#[cfg(test)]
#[path = "../../tests/apps_permissions.rs"]
mod tests;
