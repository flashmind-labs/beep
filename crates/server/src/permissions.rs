//! What Beep may do in each connected app. Reading is always allowed; tools
//! that send, change or delete follow the app's level: ask first (the
//! default), always allow, or read only. Under ask first the run ends on the
//! open call, and the user's answer resumes the session from there.

use crate::{
    apps::{CATALOG, CatalogApp, catalog_app, mcp::FASTMAIL_TOOLS},
    protocol::{AppAction, AppPermissions, Approval, ApprovalField, HistoryEntry, PermissionLevel},
    store::AppPermissionRow,
    user::User,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flashmind::{
    core::{Conversation, EntryKind},
    types::{
        ToolCall,
        tool::{InterruptPayload, Tool, ToolContext, ToolResult},
    },
};
use serde_json::Value;
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Most argument fields shown on an approval, and the longest value.
const MAX_FIELDS: usize = 8;
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

/// Composio tags of tools that create or destroy something. Such a tool
/// asks first even if the catalog lists it as a read.
const ACTING_HINTS: &[&str] = &["createHint", "destructiveHint"];

/// Whether a Composio tool's tags say it creates or destroys something.
pub(crate) fn hinted_to_act(tags: &[String]) -> bool {
    tags.iter().any(|tag| ACTING_HINTS.contains(&tag.as_str()))
}

/// A tool's name as rules keep it. Composio's tool wrappers lowercase the
/// slug, so `outlook_reply_email` is `OUTLOOK_REPLY_EMAIL`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ToolSlug(String);

impl ToolSlug {
    pub(crate) fn new(tool: &str) -> Self {
        Self(tool.to_ascii_uppercase())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The app a tool belongs to and whether it acts, for Composio slugs such
/// as `GMAIL_SEND_EMAIL` and MCP tools such as `FASTMAIL_DELETE_EMAIL`, in
/// either case.
fn lookup(tool: &str) -> Option<(&'static CatalogApp, Acts)> {
    let tool = ToolSlug::new(tool);
    let tool = tool.as_str();
    for app in CATALOG {
        if let Some(picked) = app.tools.iter().find(|picked| picked.slug == tool) {
            return Some((app, picked.acts));
        }
    }
    let name = tool.strip_prefix("FASTMAIL_")?.to_lowercase();
    let &(_, acts) = FASTMAIL_TOOLS.iter().find(|(known, _)| *known == name)?;
    Some((catalog_app("fastmail")?, acts))
}

/// The app of `tool` and whether a call with `args` acts, for a call made
/// outside a session, such as from a group. `None` for a tool outside the
/// catalog.
pub(crate) fn call_acts(tool: &str, args: &Value) -> Option<(&'static CatalogApp, bool)> {
    let (app, acts) = lookup(tool)?;
    Some((app, acts.on(args)))
}

/// The card that asks to allow `tool` with `args`.
pub(crate) fn approval_for(app: &CatalogApp, tool: &str, args: &Value) -> Approval {
    Approval {
        app: app.name.to_owned(),
        action: action_name(app, ToolSlug::new(tool).as_str()),
        fields: fields(args),
    }
}

/// The tools of `app` that can act, by the name the agent calls them.
fn action_tools(app: &CatalogApp) -> Vec<String> {
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
fn action_name(app: &CatalogApp, tool: &str) -> String {
    let prefix = format!("{}_", app.slug.to_uppercase());
    tool.strip_prefix(&prefix)
        .unwrap_or(tool)
        .to_lowercase()
        .replace('_', " ")
}

fn capitalize(text: &str) -> String {
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

impl Rules {
    pub(crate) fn from_rows(rows: Vec<AppPermissionRow>) -> Self {
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

    fn get(&self, app: &str) -> Rule {
        self.0.get(app).cloned().unwrap_or_default()
    }

    /// Whether `app` may only read.
    pub(crate) fn read_only(&self, app: &str) -> bool {
        self.get(app).level == PermissionLevel::ReadOnly {}
    }
}

/// What the phone shows for `app`.
pub(crate) fn app_permissions(app: &CatalogApp, rules: &Rules) -> AppPermissions {
    let rule = rules.get(app.slug);
    let actions = action_tools(app)
        .into_iter()
        .map(|tool| AppAction {
            label: capitalize(&action_name(app, &tool)),
            allowed: rule.allowed.contains(&tool),
            tool,
        })
        .collect();
    AppPermissions {
        level: rule.level,
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

struct Shared {
    rules: Rules,
    approved: Approved,
}

/// The app of `tool` and whether it acts, by the catalog or by its Composio
/// tags in `hinted`, the slugs `hinted_to_act` picked out. Tags only add
/// asking: a catalog action whose tags say update still asks.
fn classify(tool: &str, hinted: &HashSet<ToolSlug>) -> Option<(&'static CatalogApp, Acts)> {
    let (app, acts) = lookup(tool)?;
    if hinted.contains(&ToolSlug::new(tool)) {
        return Some((app, Acts::Always));
    }
    Some((app, acts))
}

/// Puts the app tools under the user's rules: under read only the tools that
/// always act are left out, and the rest that can act are wrapped so they
/// ask first or refuse.
pub(crate) fn guard(
    tools: Vec<Arc<dyn Tool>>,
    rules: Rules,
    approved: Approved,
    hinted: &HashSet<ToolSlug>,
) -> Vec<Arc<dyn Tool>> {
    let shared = Arc::new(Shared { rules, approved });
    tools
        .into_iter()
        .filter_map(|tool| {
            let Some((app, acts)) =
                classify(tool.name(), hinted).filter(|(_, acts)| acts.can_act())
            else {
                return Some(tool);
            };
            let read_only = shared.rules.get(app.slug).level == PermissionLevel::ReadOnly {};
            if read_only && acts == Acts::Always {
                return None;
            }
            let slug = ToolSlug::new(tool.name());
            let rule = shared.rules.get(app.slug);
            // The model only drafts in a send tool when it knows the user sees the call first.
            let description = if acts == Acts::Always
                && rule.level == (PermissionLevel::Ask {})
                && !rule.allowed.contains(slug.as_str())
            {
                format!("{} {ASKS_FIRST}", tool.description())
            } else {
                tool.description().to_owned()
            };
            Some(Arc::new(Guarded {
                slug,
                description,
                tool,
                app,
                acts,
                shared: shared.clone(),
            }) as Arc<dyn Tool>)
        })
        .collect()
}

/// Added to the description of a tool that waits for the user's OK.
const ASKS_FIRST: &str = "Beep shows the user this call, with its full arguments, \
     and it runs only once they allow it.";

struct Guarded {
    tool: Arc<dyn Tool>,
    slug: ToolSlug,
    description: String,
    app: &'static CatalogApp,
    acts: Acts,
    shared: Arc<Shared>,
}

/// What a guarded call stops the run with: the session parks on the open
/// call until the user answers.
#[derive(Debug)]
pub(crate) struct ApprovalPayload {
    pub(crate) approval: Approval,
}

impl InterruptPayload for ApprovalPayload {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn display_output(&self) -> String {
        "Waiting for the user's OK.".to_owned()
    }
}

/// The result recorded for a call the user denied.
pub(crate) const DENIED: &str =
    "Not done: the user denied it. Say so briefly and do not try again unless they ask.";
/// The result recorded for a call left unanswered when the user wrote again.
pub(crate) const SKIPPED: &str = "Not done: the user sent a new message instead of answering. \
     Do not retry unless they ask.";

/// The tool calls of the conversation's last model turn that have no result
/// yet: the call the session stopped on to ask, and the ones after it.
pub(crate) fn open_calls(conversation: &Conversation) -> Vec<ToolCall> {
    let entries = conversation.entries();
    let Some(at) = entries.iter().rposition(|entry| {
        matches!(&entry.kind, EntryKind::Assistant { tool_calls: Some(calls), .. } if !calls.is_empty())
    }) else {
        return Vec::new();
    };
    let EntryKind::Assistant {
        tool_calls: Some(calls),
        ..
    } = &entries[at].kind
    else {
        return Vec::new();
    };
    let answered: HashSet<&str> = entries[at + 1..]
        .iter()
        .filter_map(|entry| match &entry.kind {
            EntryKind::Tool { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    calls
        .iter()
        .filter(|call| !answered.contains(call.id.as_str()))
        .cloned()
        .collect()
}

/// The card for the call the session stopped on to ask, when it has one:
/// its first open call, the one an answer resumes.
pub(crate) fn waiting_approval(conversation: &Conversation) -> Option<HistoryEntry> {
    let call = open_calls(conversation).into_iter().next()?;
    let (app, _) = lookup(&call.name)?;
    let mut fields = fields(&call.arguments);
    fields.extend(records(&call.arguments, conversation));
    Some(HistoryEntry::Approval {
        approval: Approval {
            app: app.name.to_owned(),
            action: action_name(app, ToolSlug::new(&call.name).as_str()),
            fields: fields.into_iter().take(MAX_FIELDS).collect(),
        },
        query_id: call.id,
    })
}

/// What the records `call_id` names are, such as the subject and sender of
/// the email behind a message ID, read from earlier tool results in
/// `conversation`. Empty when the call names none or nothing said more.
pub(crate) fn describe_records(call_id: &str, conversation: &Conversation) -> Vec<ApprovalField> {
    let call = conversation
        .entries()
        .iter()
        .rev()
        .find_map(|entry| match &entry.kind {
            EntryKind::Assistant {
                tool_calls: Some(calls),
                ..
            } => calls.iter().find(|call| call.id == call_id),
            _ => None,
        });
    call.map(|call| records(&call.arguments, conversation))
        .unwrap_or_default()
}

/// The fields of a record that tell the user which one it is.
const RECORD_KEYS: &[(&str, &str)] = &[
    ("subject", "Subject"),
    ("title", "Title"),
    ("name", "Name"),
    ("from", "From"),
    ("sender", "From"),
];

fn records(args: &Value, conversation: &Conversation) -> Vec<ApprovalField> {
    let Value::Object(args) = args else {
        return Vec::new();
    };
    let ids: Vec<&str> = args
        .iter()
        .filter(|(key, _)| is_id(key))
        .flat_map(|(_, value)| match value {
            Value::String(id) => vec![id.as_str()],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        })
        .filter(|id| !id.trim().is_empty())
        .collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let results: Vec<Value> = conversation
        .entries()
        .iter()
        .rev()
        .filter_map(|entry| match &entry.kind {
            EntryKind::Tool { output, .. } => parse_output(output),
            _ => None,
        })
        .collect();
    let mut shown = Vec::new();
    for id in ids {
        let Some(record) = results.iter().find_map(|result| find_record(result, id)) else {
            continue;
        };
        let mut labels = HashSet::new();
        for (key, label) in RECORD_KEYS {
            let Some(value) = record_value(record, key) else {
                continue;
            };
            if labels.insert(*label) {
                shown.push(ApprovalField {
                    label: (*label).to_owned(),
                    value: value.chars().take(MAX_VALUE_CHARS).collect(),
                });
            }
        }
    }
    shown.into_iter().take(MAX_FIELDS).collect()
}

/// A tool result as JSON, skipping any text before it.
fn parse_output(output: &str) -> Option<Value> {
    let start = output.find(['{', '['])?;
    serde_json::from_str(&output[start..]).ok()
}

/// The object in `value` that has `id` as one of its own values.
fn find_record<'a>(value: &'a Value, id: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if map.values().any(|v| v.as_str() == Some(id)) {
                return Some(value);
            }
            map.values().find_map(|v| find_record(v, id))
        }
        Value::Array(items) => items.iter().find_map(|v| find_record(v, id)),
        _ => None,
    }
}

/// `key` of `record` as text: a plain string, an address, or a list of
/// addresses, as Gmail, Outlook and Fastmail write a sender.
fn record_value(record: &Value, key: &str) -> Option<String> {
    let value = record
        .as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))?
        .1;
    let text = match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| address(item).or_else(|| address(item.get("emailAddress")?)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => address(value).or_else(|| address(value.get("emailAddress")?))?,
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// Stops asking for `tool` in its app.
pub(crate) async fn allow_always(user: &User, tool: &str) -> Result<()> {
    let Some((app, _)) = lookup(tool) else {
        return Ok(());
    };
    let rules = user.app_rules().await?;
    let mut rule = rules.get(app.slug);
    rule.allowed.insert(ToolSlug::new(tool).0);
    user.save_app_permissions(&row(app, rule.level, rule.allowed.into_iter().collect()))
        .await
}

/// The call's arguments as the user reads them.
fn fields(args: &Value) -> Vec<ApprovalField> {
    let Value::Object(args) = args else {
        return Vec::new();
    };
    args.iter()
        .filter(|(key, _)| !is_id(key))
        .filter_map(|(key, value)| {
            let value = match value {
                Value::Null => return None,
                Value::String(text) => text.trim().to_owned(),
                Value::Array(items) if items.iter().all(|item| address(item).is_some()) => items
                    .iter()
                    .filter_map(address)
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            if value.is_empty() || value == "[]" || value == "{}" {
                return None;
            }
            let mut value: String = value.chars().take(MAX_VALUE_CHARS).collect();
            if value.chars().count() == MAX_VALUE_CHARS {
                value.push_str("...");
            }
            Some(ApprovalField {
                label: capitalize(&key.replace('_', " ")),
                value,
            })
        })
        .take(MAX_FIELDS)
        .collect()
}

/// A recipient written as `{"name": "Jo", "email": "jo@example.com"}`, as
/// Fastmail takes them, read as "Jo <jo@example.com>".
fn address(item: &Value) -> Option<String> {
    let email = item
        .get("email")
        .or_else(|| item.get("address"))?
        .as_str()?;
    Some(match item.get("name").and_then(Value::as_str) {
        Some(name) if !name.trim().is_empty() => format!("{} <{email}>", name.trim()),
        _ => email.to_owned(),
    })
}

/// An argument that only names a record, such as `message_id` or `emailId`,
/// whose value means nothing to the user.
fn is_id(key: &str) -> bool {
    let camel = key.ends_with("Id") || key.ends_with("Ids");
    let key = key.to_ascii_lowercase();
    camel || ["id", "ids"].contains(&key.as_str()) || key.ends_with("_id") || key.ends_with("_ids")
}

#[async_trait]
impl Tool for Guarded {
    fn name(&self) -> &str {
        self.tool.name()
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.tool.parameters()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        if !self.acts.on(&ctx.args) {
            return self.tool.execute(ctx).await;
        }
        let name = self.slug.as_str();
        let rule = self.shared.rules.get(self.app.slug);
        let app = self.app.name;
        match rule.level {
            PermissionLevel::Allow {} => self.tool.execute(ctx).await,
            PermissionLevel::ReadOnly {} => Ok(ToolResult::success(
                ctx.tool_call_id,
                format!(
                    "Not done: the user set {app} to read only. Tell them, and that they \
                     can change it under Apps in Beep's menu."
                ),
            )),
            PermissionLevel::Ask {}
                if rule.allowed.contains(name)
                    || self.shared.approved.0.as_deref() == Some(ctx.tool_call_id) =>
            {
                self.tool.execute(ctx).await
            }
            PermissionLevel::Ask {} => {
                let approval = Approval {
                    app: app.to_owned(),
                    action: action_name(self.app, name),
                    fields: fields(&ctx.args),
                };
                Ok(ToolResult::interrupt(
                    ctx.tool_call_id,
                    Arc::new(ApprovalPayload { approval }),
                ))
            }
        }
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.tool.timeout_secs()
    }

    fn max_output_bytes(&self) -> usize {
        self.tool.max_output_bytes()
    }

    fn max_output_lines(&self) -> usize {
        self.tool.max_output_lines()
    }

    fn humanize(&self, args: &Value) -> String {
        self.tool.humanize(args)
    }

    fn concurrent(&self) -> bool {
        self.tool.concurrent()
    }
}

/// Longest field value an action's description keeps.
const MAX_ACTION_VALUE_CHARS: usize = 80;
/// Fields an action's description keeps.
const MAX_ACTION_FIELDS: usize = 4;

/// What the turn did in the user's apps from `since` on, each as one line
/// such as "Gmail: send email (Recipient email: jo@example.com; Subject:
/// Lunch)". Only calls that act and went through: denied, skipped and
/// failed calls are left out.
pub(crate) fn actions_since(conversation: &Conversation, since: DateTime<Utc>) -> Vec<String> {
    let entries = conversation.entries();
    let calls: HashMap<&str, &ToolCall> = entries
        .iter()
        .filter_map(|entry| match &entry.kind {
            EntryKind::Assistant {
                tool_calls: Some(calls),
                ..
            } => Some(calls),
            _ => None,
        })
        .flatten()
        .map(|call| (call.id.as_str(), call))
        .collect();
    entries
        .iter()
        .filter(|entry| entry.timestamp >= since)
        .filter_map(|entry| match &entry.kind {
            EntryKind::Tool { call_id, output } => Some((calls.get(call_id.as_str())?, output)),
            _ => None,
        })
        .filter(|(_, output)| went_through(output))
        .filter_map(|(call, _)| {
            let (app, acts) = lookup(&call.name)?;
            acts.on(&call.arguments)
                .then(|| describe_action(app, call, conversation))
        })
        .collect()
}

/// Whether a tool result says the call did what it was asked.
fn went_through(output: &str) -> bool {
    if output == DENIED || output == SKIPPED {
        return false;
    }
    let failed = parse_output(output).is_some_and(|result| {
        result.get("successful") == Some(&Value::Bool(false))
            || result.get("error").is_some_and(|error| !error.is_null())
    });
    !failed
}

fn describe_action(app: &CatalogApp, call: &ToolCall, conversation: &Conversation) -> String {
    let action = action_name(app, ToolSlug::new(&call.name).as_str());
    let mut fields = records(&call.arguments, conversation);
    fields.extend(fields_of(&call.arguments));
    let details: Vec<String> = fields
        .into_iter()
        .take(MAX_ACTION_FIELDS)
        .map(|field| {
            let value: String = field.value.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut value: String = value.chars().take(MAX_ACTION_VALUE_CHARS).collect();
            if value.chars().count() == MAX_ACTION_VALUE_CHARS {
                value.push_str("...");
            }
            format!("{}: {value}", field.label)
        })
        .collect();
    if details.is_empty() {
        format!("{}: {action}", app.name)
    } else {
        format!("{}: {action} ({})", app.name, details.join("; "))
    }
}

/// The argument fields that say most about an action: who and what, before
/// long text such as a body.
fn fields_of(args: &Value) -> Vec<ApprovalField> {
    let mut fields = fields(args);
    fields.sort_by_key(|field| field.value.chars().count() > MAX_ACTION_VALUE_CHARS);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::core::ConversationEntry;
    use serde_json::json;

    #[test]
    fn tools_are_classified_by_app() {
        let (app, acts) = lookup("GMAIL_SEND_EMAIL").unwrap();
        assert_eq!((app.slug, acts), ("gmail", Acts::Always));
        assert_eq!(lookup("GMAIL_FETCH_EMAILS").unwrap().1, Acts::Never);
        let (app, acts) = lookup("FASTMAIL_UPDATE_EMAIL").unwrap();
        assert_eq!((app.slug, acts), ("fastmail", Acts::WhenMoving));
        assert!(lookup("open_maps").is_none());
    }

    #[test]
    fn lowercase_composio_names_are_classified() {
        let none = HashSet::new();
        let (app, acts) = classify("outlook_reply_email", &none).unwrap();
        assert_eq!((app.slug, acts), ("outlook", Acts::Always));
        assert_eq!(
            classify("outlook_query_emails", &none).unwrap().1,
            Acts::Never
        );
    }

    #[test]
    fn hints_add_asking() {
        assert!(hinted_to_act(&[
            "openWorldHint".into(),
            "createHint".into()
        ]));
        assert!(hinted_to_act(&["destructiveHint".into()]));
        assert!(!hinted_to_act(&[
            "updateHint".into(),
            "openWorldHint".into()
        ]));
        let hinted: HashSet<ToolSlug> = [ToolSlug::new("GMAIL_FETCH_EMAILS")].into();
        assert_eq!(
            classify("gmail_fetch_emails", &hinted).unwrap().1,
            Acts::Always
        );
        // An update-only catalog action keeps asking without a hint.
        assert_eq!(
            classify("gmail_move_to_trash", &HashSet::new()).unwrap().1,
            Acts::Always
        );
    }

    #[test]
    fn moving_mail_acts() {
        let update = Acts::WhenMoving;
        assert!(!update.on(&json!({"ids": ["M1"], "isRead": true})));
        assert!(update.on(&json!({"ids": ["M1"], "folder": "Trash"})));
    }

    #[test]
    fn actions_are_listed_with_labels() {
        let gmail = catalog_app("gmail").unwrap();
        let mut rules = Rules::default();
        rules.0.insert(
            "gmail".into(),
            Rule {
                level: PermissionLevel::Ask {},
                allowed: ["GMAIL_REPLY_TO_THREAD".to_owned()].into(),
            },
        );
        let permissions = app_permissions(gmail, &rules);
        let labels: Vec<(&str, bool)> = permissions
            .actions
            .iter()
            .map(|action| (action.label.as_str(), action.allowed))
            .collect();
        assert_eq!(
            labels,
            [
                ("Send email", false),
                ("Reply to thread", true),
                ("Move to trash", false)
            ]
        );
        let fastmail = app_permissions(catalog_app("fastmail").unwrap(), &Rules::default());
        assert!(
            fastmail
                .actions
                .iter()
                .any(|a| a.tool == "FASTMAIL_DELETE_EMAIL")
        );
        assert!(
            !fastmail
                .actions
                .iter()
                .any(|a| a.tool == "FASTMAIL_READ_EMAIL")
        );
    }

    #[test]
    fn saved_rows_keep_only_known_actions() {
        let gmail = catalog_app("gmail").unwrap();
        let row = row(
            gmail,
            PermissionLevel::Ask {},
            vec![
                "GMAIL_SEND_EMAIL".into(),
                "GMAIL_FETCH_EMAILS".into(),
                "X".into(),
            ],
        );
        assert_eq!(row.allowed, ["GMAIL_SEND_EMAIL"]);
    }

    #[test]
    fn open_calls_are_the_unanswered_ones_of_the_last_turn() {
        let call = |id: &str| ToolCall {
            id: id.into(),
            name: "GMAIL_SEND_EMAIL".into(),
            arguments: json!({}),
        };
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user("hi"));
        assert!(open_calls(&conversation).is_empty());
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![call("a"), call("b"), call("c")],
            None,
        ));
        conversation.add(ConversationEntry::tool("a", "sent"));
        let open: Vec<String> = open_calls(&conversation)
            .into_iter()
            .map(|call| call.id)
            .collect();
        assert_eq!(open, ["b", "c"]);
        conversation.add(ConversationEntry::tool("b", "sent"));
        conversation.add(ConversationEntry::tool("c", "sent"));
        assert!(open_calls(&conversation).is_empty());
    }

    #[test]
    fn a_session_stopped_on_a_call_shows_its_card() {
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user("Send the email"));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "send".into(),
                name: "gmail_send_email".into(),
                arguments: json!({"recipient_email": "a@example.com", "subject": "Stay"}),
            }],
            None,
        ));
        let Some(HistoryEntry::Approval { query_id, approval }) = waiting_approval(&conversation)
        else {
            panic!("expected the open call's card");
        };
        assert_eq!(query_id, "send");
        assert_eq!(
            (approval.app.as_str(), approval.action.as_str()),
            ("Gmail", "send email")
        );
        assert_eq!(approval.fields.len(), 2);
        conversation.add(ConversationEntry::tool("send", DENIED));
        assert!(waiting_approval(&conversation).is_none());
    }

    #[test]
    fn a_card_says_which_email_an_id_names() {
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "find".into(),
                name: "gmail_fetch_emails".into(),
                arguments: json!({"query": "in:inbox"}),
            }],
            None,
        ));
        let found = json!({"data": {"messages": [
            {"messageId": "m1", "subject": "Renew example.com", "sender": "Registrar <billing@example.com>"},
            {"messageId": "m2", "subject": "New listings", "sender": "alerts@example.com"},
        ]}});
        conversation.add(ConversationEntry::tool("find", found.to_string()));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "trash".into(),
                name: "gmail_move_to_trash".into(),
                arguments: json!({"message_id": "m2"}),
            }],
            None,
        ));
        let Some(HistoryEntry::Approval { approval, .. }) = waiting_approval(&conversation) else {
            panic!("expected the open call's card");
        };
        let shown: Vec<_> = approval
            .fields
            .iter()
            .map(|f| (f.label.as_str(), f.value.as_str()))
            .collect();
        assert_eq!(
            shown,
            [("Subject", "New listings"), ("From", "alerts@example.com")]
        );
        assert_eq!(describe_records("trash", &conversation).len(), 2);
    }

    #[test]
    fn records_read_outlook_senders() {
        let record = json!({"from": {"emailAddress": {"name": "Jo", "address": "jo@example.com"}}});
        assert_eq!(
            record_value(&record, "from").as_deref(),
            Some("Jo <jo@example.com>")
        );
    }

    #[test]
    fn fields_skip_empty_values() {
        let shown = fields(&json!({"recipient_email": "a@example.com", "cc": [], "body": null}));
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].label, "Recipient email");
    }

    #[test]
    fn fields_skip_ids() {
        let shown =
            fields(&json!({"message_id": "AAMk", "ids": ["M1"], "emailId": "M2", "comment": "Hi"}));
        let labels: Vec<&str> = shown.iter().map(|field| field.label.as_str()).collect();
        assert_eq!(labels, ["Comment"]);
    }

    #[test]
    fn fields_read_recipients() {
        let to = json!([{"name": "Jo", "email": "jo@example.com"}, {"email": "sam@example.com"}]);
        let shown = fields(&json!({ "to": to }));
        assert_eq!(shown[0].value, "Jo <jo@example.com>, sam@example.com");
    }

    #[test]
    fn actions_are_the_calls_that_acted_and_went_through() {
        let call = |id: &str, name: &str, arguments| ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        };
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user("Email Jo about lunch"));
        let since = conversation.entries()[0].timestamp;
        let email = json!({"recipient_email": "jo@example.com", "subject": "Lunch", "body": "See you at noon."});
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![
                call("read", "GMAIL_FETCH_EMAILS", json!({})),
                call("sent", "GMAIL_SEND_EMAIL", email.clone()),
                call("denied", "GMAIL_SEND_EMAIL", email.clone()),
                call("failed", "GMAIL_SEND_EMAIL", email),
            ],
            None,
        ));
        conversation.add(ConversationEntry::tool("read", "[]"));
        conversation.add(ConversationEntry::tool("sent", r#"{"successful": true}"#));
        conversation.add(ConversationEntry::tool("denied", DENIED));
        conversation.add(ConversationEntry::tool(
            "failed",
            r#"{"successful": false, "error": "quota"}"#,
        ));
        let actions = actions_since(&conversation, since);
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert!(actions[0].starts_with("Gmail: "), "{}", actions[0]);
        assert!(actions[0].contains("jo@example.com"), "{}", actions[0]);
        assert!(actions[0].contains("Lunch"), "{}", actions[0]);
    }
}
