//! What Beep may do in each connected app. Reading is always allowed; tools
//! that send, change or delete follow the app's level: ask first (the
//! default), always allow, or read only. Under ask first the run ends on the
//! open call, and the user's answer resumes the session from there.
//!
//! Background work (sub-agents and jobs) has no turn to end, so its calls
//! pass a [`Gate`] instead: the same rules, and under ask first the same
//! card, with only that call waiting for the answer.

use crate::{
    apps::{
        CATALOG, CatalogApp, catalog_app, custom_mcp::PREFIX as ADDED_PREFIX, mcp::FASTMAIL_TOOLS,
    },
    grants::RoutineGrants,
    protocol::{AppAction, AppPermissions, Approval, ApprovalField, HistoryEntry, PermissionLevel},
    store::AppPermissionRow,
    subagents::Approvals,
    user::User,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flashmind::{
    core::{Conversation, ConversationEntry, EntryKind},
    types::{
        GateAsk, GateDecision, ToolCall, ToolGate,
        tool::{InterruptPayload, Tool, ToolContext, ToolResult},
    },
};
use serde_json::{Map, Value};
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

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

/// The app a tool belongs to: one in the catalog, or a server the user
/// added, by its row name such as `mcp_my-notes`.
#[derive(Clone)]
pub(crate) enum ToolApp {
    Catalog(&'static CatalogApp),
    Added(String),
}

impl ToolApp {
    pub(crate) fn slug(&self) -> &str {
        match self {
            ToolApp::Catalog(app) => app.slug,
            ToolApp::Added(slug) => slug,
        }
    }

    /// The app's name on an approval: "My notes" for `mcp_my-notes`.
    pub(crate) fn name(&self) -> String {
        match self {
            ToolApp::Catalog(app) => app.name.to_owned(),
            ToolApp::Added(slug) => {
                capitalize(&slug.trim_start_matches(ADDED_PREFIX).replace('-', " "))
            }
        }
    }
}

/// The app a tool belongs to and whether it acts, for Composio slugs such
/// as `GMAIL_SEND_EMAIL`, MCP tools such as `FASTMAIL_DELETE_EMAIL`, in
/// either case, and tools of added servers such as `MCP_MY-NOTES_SEARCH`.
/// An added server says nothing Beep trusts about its tools, so they all act.
pub(crate) fn lookup(tool: &str) -> Option<(ToolApp, Acts)> {
    let tool = ToolSlug::new(tool);
    let tool = tool.as_str();
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
    let name = tool.strip_prefix("FASTMAIL_")?.to_lowercase();
    let &(_, acts) = FASTMAIL_TOOLS.iter().find(|(known, _)| *known == name)?;
    Some((ToolApp::Catalog(catalog_app("fastmail")?), acts))
}

/// The catalog app of `tool` and whether a call with `args` acts, for a
/// call made outside a session, such as from a group. `None` for a tool
/// outside the catalog.
pub(crate) fn call_acts(tool: &str, args: &Value) -> Option<(&'static CatalogApp, bool)> {
    match lookup(tool)? {
        (ToolApp::Catalog(app), acts) => Some((app, acts.on(args))),
        (ToolApp::Added(_), _) => None,
    }
}

/// The card that asks to allow `tool` with `args`.
pub(crate) fn approval_for(app: &ToolApp, tool: &str, args: &Value) -> Approval {
    Approval {
        app: app.name(),
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
pub(crate) fn action_name(app: &ToolApp, tool: &str) -> String {
    let prefix = format!("{}_", app.slug().to_uppercase());
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

    /// The level of `app`.
    fn level(&self, app: &str) -> PermissionLevel {
        self.0
            .get(app)
            .map_or(Rule::default().level, |rule| rule.level)
    }

    /// Whether `tool` of `app` runs without asking under ask first.
    fn allows(&self, app: &str, tool: &str) -> bool {
        self.0
            .get(app)
            .is_some_and(|rule| rule.allowed.contains(tool))
    }

    /// Whether `app` may only read.
    pub(crate) fn read_only(&self, app: &str) -> bool {
        self.level(app) == PermissionLevel::ReadOnly {}
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

struct Shared {
    rules: Rules,
    approved: Approved,
    /// The grants of the routine whose run this is, tried before asking.
    grants: Option<Arc<RoutineGrants>>,
}

/// The app of `tool` and whether it acts, by the catalog or by its Composio
/// tags in `hinted`, the slugs `hinted_to_act` picked out. Tags only add
/// asking: a catalog action whose tags say update still asks. A catalog
/// tool marked `over_hints` keeps its own answer.
fn classify(tool: &str, hinted: &HashSet<ToolSlug>) -> Option<(ToolApp, Acts)> {
    let (app, acts) = lookup(tool)?;
    let slug = ToolSlug::new(tool);
    let over_hints = matches!(&app, ToolApp::Catalog(catalog) if catalog
        .tools
        .iter()
        .any(|picked| picked.over_hints && picked.slug == slug.as_str()));
    if hinted.contains(&slug) && !over_hints {
        return Some((app, Acts::Always));
    }
    Some((app, acts))
}

/// The catalog app of `tool` when no call of it can act, by the catalog and
/// the tags in `hinted`; `None` for a tool that can act or that Beep does
/// not know.
pub(crate) fn read_tool_app(tool: &str, hinted: &HashSet<ToolSlug>) -> Option<&'static str> {
    match classify(tool, hinted)? {
        (ToolApp::Catalog(app), Acts::Never) => Some(app.slug),
        _ => None,
    }
}

/// Only those of `tools` that Beep knows never act, for a turn that runs on
/// untrusted text.
pub(crate) fn read_tools_only(
    tools: Vec<Arc<dyn Tool>>,
    hinted: &HashSet<ToolSlug>,
) -> Vec<Arc<dyn Tool>> {
    tools
        .into_iter()
        .filter(|tool| read_tool_app(tool.name(), hinted).is_some())
        .collect()
}

/// Puts the app tools under the user's rules: under read only the tools that
/// always act are left out, and the rest that can act are wrapped so they
/// ask first or refuse. In a routine's run, a call that would ask first
/// runs without asking when one of the routine's `grants` lets it through.
pub(crate) fn guard(
    tools: Vec<Arc<dyn Tool>>,
    rules: Rules,
    approved: Approved,
    hinted: &HashSet<ToolSlug>,
    grants: Option<Arc<RoutineGrants>>,
) -> Vec<Arc<dyn Tool>> {
    let shared = Arc::new(Shared {
        rules,
        approved,
        grants,
    });
    tools
        .into_iter()
        .filter_map(|tool| {
            let Some((app, acts)) =
                classify(tool.name(), hinted).filter(|(_, acts)| acts.can_act())
            else {
                return Some(tool);
            };
            let level = shared.rules.level(app.slug());
            if level == (PermissionLevel::ReadOnly {}) && acts == Acts::Always {
                return None;
            }
            let slug = ToolSlug::new(tool.name());
            // The model only drafts in a send tool when it knows the user sees the call first.
            let description = if acts == Acts::Always
                && level == (PermissionLevel::Ask {})
                && !shared.rules.allows(app.slug(), slug.as_str())
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

/// What the user's rules say about one call that acts.
enum Check {
    Run,
    /// Not done, with the result the model reads.
    Refuse(String),
    /// Asks the user first, with this card.
    Ask(Approval),
}

/// What the rules say about `tool`, of `app`, acting with `args`.
fn check(rules: &Rules, app: &ToolApp, tool: &str, args: &Value) -> Check {
    match rules.level(app.slug()) {
        PermissionLevel::Allow {} => Check::Run,
        PermissionLevel::ReadOnly {} => Check::Refuse(format!(
            "{READ_ONLY} {} to read only. Tell them, and that they \
             can change it under Apps in Beep's menu.",
            app.name()
        )),
        PermissionLevel::Ask {} if rules.allows(app.slug(), ToolSlug::new(tool).as_str()) => {
            Check::Run
        }
        PermissionLevel::Ask {} => Check::Ask(approval_for(app, tool, args)),
    }
}

/// The app tools background work may have: under read only the tools that
/// always act are left out. The rest are checked by a [`Gate`] as they run.
pub(crate) fn for_gate(
    tools: Vec<Arc<dyn Tool>>,
    rules: &Rules,
    hinted: &HashSet<ToolSlug>,
) -> Vec<Arc<dyn Tool>> {
    tools
        .into_iter()
        .filter(|tool| match classify(tool.name(), hinted) {
            Some((app, Acts::Always)) => !rules.read_only(app.slug()),
            _ => true,
        })
        .collect()
}

/// How long a background call waits for the user's answer before it is
/// refused.
pub(crate) const ASK_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The user's rules on the calls of background work in one chat. A call
/// that asks first shows the user the same card a turn would, attributed
/// to that chat, and waits for the answer without holding anything else up.
pub(crate) struct Gate {
    /// The rules when the work started, used when the current ones cannot
    /// be read. Each call reads the user's rules again, so an Always or a
    /// switch to read only reaches work already running.
    pub(crate) rules: Rules,
    pub(crate) hinted: HashSet<ToolSlug>,
    pub(crate) user: Arc<User>,
    /// The chat the work belongs to, which the card opens.
    pub(crate) chat: Uuid,
    pub(crate) approvals: Arc<Approvals>,
    /// The grants of the routine whose run started the work, tried before
    /// asking; `None` outside a routine's run.
    pub(crate) grants: Option<Arc<RoutineGrants>>,
}

#[async_trait]
impl ToolGate for Gate {
    async fn check(&self, call: &ToolCall) -> GateDecision {
        let Some((app, acts)) = classify(&call.name, &self.hinted) else {
            return GateDecision::Allow;
        };
        if !acts.on(&call.arguments) {
            return GateDecision::Allow;
        }
        let rules = match self.user.app_rules().await {
            Ok(rules) => rules,
            Err(error) => {
                log::warn!(
                    "permissions: unable to reload, using the rules work started with: {error:#}"
                );
                self.rules.clone()
            }
        };
        match check(&rules, &app, &call.name, &call.arguments) {
            Check::Run => GateDecision::Allow,
            Check::Refuse(reason) => GateDecision::Deny(reason),
            Check::Ask(mut approval) => {
                if let Some(grants) = &self.grants
                    && let Some(grant) = grants.claim(&call.name, &call.arguments, &call.id).await
                {
                    // The gate cannot see the result, so the user hears of
                    // the call as it is let through.
                    grants.report(&grant, &call.name, &call.arguments);
                    return GateDecision::Allow;
                }
                approval.fields.extend(read_back(&self.user, call).await);
                GateDecision::Ask(GateAsk {
                    answer: self
                        .approvals
                        .ask(self.user.id(), self.chat, &call.name, approval),
                    timeout: ASK_TIMEOUT,
                })
            }
        }
    }
}

/// Added to the description of a tool that waits for the user's OK.
const ASKS_FIRST: &str = "Beep shows the user this call, with its full arguments, \
     and it runs only once they allow it.";

struct Guarded {
    tool: Arc<dyn Tool>,
    slug: ToolSlug,
    description: String,
    app: ToolApp,
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

/// The result recorded for a call left unanswered when a routine's turn
/// showed in the chat, which takes its card away.
pub(crate) const SUPERSEDED: &str = "Not done: a routine ran in this chat before the user \
     answered, and the question was dropped. Do not retry unless they ask.";

/// How the result of a call refused under read only starts.
const READ_ONLY: &str = "Not done: the user set";

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

/// Records `output` as the result of every open call of `conversation`, so
/// none is left without one once its card is gone.
pub(crate) fn close_open_calls(conversation: &mut Conversation, output: &str) {
    for call in open_calls(conversation) {
        conversation.add(ConversationEntry::tool(&call.id, output));
    }
}

/// The card for the call the session stopped on to ask, when it has one:
/// its first open call, the one an answer resumes.
pub(crate) fn waiting_approval(conversation: &Conversation) -> Option<HistoryEntry> {
    let call = open_calls(conversation).into_iter().next()?;
    let (app, _) = lookup(&call.name)?;
    let mut fields = fields(&call.arguments);
    fields.extend(records(&call, conversation));
    Some(HistoryEntry::Approval {
        approval: Approval {
            app: app.name(),
            action: action_name(&app, ToolSlug::new(&call.name).as_str()),
            fields: fields.into_iter().take(MAX_FIELDS).collect(),
        },
        query_id: call.id,
    })
}

/// What the records `call_id` names are, such as the subject and sender of
/// the email behind a message ID, read from earlier tool results in
/// `conversation`. Empty when the call names none or nothing said more.
pub(crate) fn describe_records(call_id: &str, conversation: &Conversation) -> Vec<ApprovalField> {
    find_call(call_id, conversation)
        .map(|call| records(call, conversation))
        .unwrap_or_default()
}

/// The call `call_id` in `conversation`.
fn find_call<'a>(call_id: &str, conversation: &'a Conversation) -> Option<&'a ToolCall> {
    conversation
        .entries()
        .iter()
        .rev()
        .find_map(|entry| match &entry.kind {
            EntryKind::Assistant {
                tool_calls: Some(calls),
                ..
            } => calls.iter().find(|call| call.id == call_id),
            _ => None,
        })
}

/// The fields of a record that tell the user which one it is.
const RECORD_KEYS: &[(&str, &str)] = &[
    ("subject", "Subject"),
    ("title", "Title"),
    ("summary", "Title"),
    ("content", "Task"),
    ("name", "Name"),
    ("from", "From"),
    ("sender", "From"),
];

fn records(call: &ToolCall, conversation: &Conversation) -> Vec<ApprovalField> {
    let ids = record_ids(&call.arguments);
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
    ids.into_iter()
        .filter_map(|id| results.iter().find_map(|result| find_record(result, id)))
        .flat_map(|record| record_fields(record, call))
        .take(MAX_FIELDS)
        .collect()
}

/// Most records a card reads for one call.
const MAX_FETCHED: usize = 3;

/// How to read what an acting call names, for a call that only gives IDs.
struct Lookup {
    /// The acting tools, as rules name them.
    tools: &'static [&'static str],
    /// The tool that reads one record, Fastmail's by their MCP name.
    read: &'static str,
    /// Arguments carried from the call to the read, as (call's, read's). A
    /// list, such as Fastmail's `ids`, is read one item at a time.
    args: &'static [(&'static str, &'static str)],
}

/// Every acting tool that names a record a tool can read back. Fastmail has
/// no tool that reads one event or contact, and Slack, Teams and SharePoint
/// calls name channels and lists rather than one record.
const LOOKUPS: &[Lookup] = &[
    Lookup {
        tools: &[
            "FASTMAIL_DELETE_EMAIL",
            "FASTMAIL_ARCHIVE_EMAIL",
            "FASTMAIL_UPDATE_EMAIL",
        ],
        read: "read_email",
        args: &[("ids", "id")],
    },
    Lookup {
        tools: &["FASTMAIL_SEND_EMAIL"],
        read: "read_email",
        args: &[("emailId", "id")],
    },
    Lookup {
        tools: &["GMAIL_MOVE_TO_TRASH"],
        read: "GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID",
        args: &[("message_id", "message_id")],
    },
    Lookup {
        tools: &["GMAIL_BATCH_MODIFY_MESSAGES"],
        read: "GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID",
        args: &[("messageIds", "message_id")],
    },
    Lookup {
        tools: &["GMAIL_REPLY_TO_THREAD"],
        read: "GMAIL_FETCH_MESSAGE_BY_THREAD_ID",
        args: &[("thread_id", "thread_id")],
    },
    Lookup {
        tools: &["GOOGLECALENDAR_PATCH_EVENT", "GOOGLECALENDAR_DELETE_EVENT"],
        read: "GOOGLECALENDAR_EVENTS_GET",
        args: &[("event_id", "event_id"), ("calendar_id", "calendar_id")],
    },
    Lookup {
        tools: &["OUTLOOK_REPLY_EMAIL", "OUTLOOK_MOVE_MESSAGE"],
        read: "OUTLOOK_GET_MESSAGE",
        args: &[("message_id", "message_id")],
    },
    Lookup {
        tools: &[
            "OUTLOOK_ACCEPT_EVENT",
            "OUTLOOK_DECLINE_EVENT",
            "OUTLOOK_UPDATE_CALENDAR_EVENT",
            "OUTLOOK_DELETE_CALENDAR_EVENT",
        ],
        read: "OUTLOOK_GET_EVENT",
        args: &[("event_id", "event_id")],
    },
    Lookup {
        tools: &["NOTION_UPDATE_PAGE"],
        read: "NOTION_RETRIEVE_PAGE",
        args: &[("page_id", "page_id")],
    },
    Lookup {
        tools: &["NOTION_ADD_MULTIPLE_PAGE_CONTENT"],
        read: "NOTION_RETRIEVE_PAGE",
        args: &[("parent_block_id", "page_id")],
    },
    Lookup {
        tools: &["LINEAR_UPDATE_ISSUE", "LINEAR_CREATE_LINEAR_COMMENT"],
        read: "LINEAR_GET_LINEAR_ISSUE",
        args: &[("issueId", "issue_id"), ("issue_id", "issue_id")],
    },
    Lookup {
        tools: &["TODOIST_UPDATE_TASK", "TODOIST_CLOSE_TASK"],
        read: "TODOIST_GET_TASK",
        args: &[("task_id", "task_id")],
    },
    Lookup {
        tools: &["ONE_DRIVE_MOVE_ITEM"],
        read: "ONE_DRIVE_GET_ITEM",
        args: &[("itemId", "item_id"), ("driveId", "drive_id")],
    },
    Lookup {
        tools: &["ONE_DRIVE_CREATE_LINK"],
        read: "ONE_DRIVE_GET_ITEM",
        args: &[("item_id", "item_id"), ("drive_id", "drive_id")],
    },
    Lookup {
        tools: &["GITHUB_CREATE_AN_ISSUE_COMMENT"],
        read: "GITHUB_GET_AN_ISSUE",
        args: &[
            ("owner", "owner"),
            ("repo", "repo"),
            ("issue_number", "issue_number"),
        ],
    },
];

/// What the records of call `call_id` are: from earlier results in
/// `conversation`, or else read back with the app's own tool, so a delete
/// that only names an ID still says what it deletes.
pub(crate) async fn describe_call(
    user: &User,
    call_id: &str,
    conversation: &Conversation,
) -> Vec<ApprovalField> {
    let shown = describe_records(call_id, conversation);
    if !shown.is_empty() {
        return shown;
    }
    match find_call(call_id, conversation) {
        Some(call) => read_back(user, call).await,
        None => Vec::new(),
    }
}

/// What the records `call` names are, read back with the app's own tool.
pub(crate) async fn read_back(user: &User, call: &ToolCall) -> Vec<ApprovalField> {
    let tool = ToolSlug::new(&call.name);
    let Some(lookup) = LOOKUPS
        .iter()
        .find(|lookup| lookup.tools.contains(&tool.as_str()))
    else {
        return Vec::new();
    };
    // A Fastmail call reads back from the account it acts in.
    let account = call.arguments.get("account").and_then(Value::as_str);
    let mut shown = Vec::new();
    for args in read_args(lookup, &call.arguments) {
        match read_record(user, lookup.read, account, args.clone()).await {
            Ok(found) => {
                // The record is the object holding the ID it was read by;
                // a read that answers with the record alone is that object.
                let id = args.values().find_map(Value::as_str).unwrap_or_default();
                shown.extend(record_fields(
                    find_record(&found, id).unwrap_or(&found),
                    call,
                ));
            }
            Err(error) => log::debug!("approval: unable to read {}: {error:#}", lookup.read),
        }
    }
    shown.into_iter().take(MAX_FIELDS).collect()
}

/// The arguments for each read `lookup` makes for a call with `args`: one,
/// or one per item when the call names a list. None when the call left out
/// what the read needs.
fn read_args(lookup: &Lookup, args: &Value) -> Vec<Map<String, Value>> {
    let mut shared = Map::new();
    let mut listed: Option<(&str, &Vec<Value>)> = None;
    for &(from, to) in lookup.args {
        match args.get(from) {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => listed = Some((to, items)),
            Some(value) => {
                shared.insert(to.to_owned(), value.clone());
            }
        }
    }
    let Some((to, items)) = listed else {
        return if shared.is_empty() {
            Vec::new()
        } else {
            vec![shared]
        };
    };
    items
        .iter()
        .filter(|item| item.as_str().is_some_and(|id| !id.trim().is_empty()))
        .take(MAX_FETCHED)
        .map(|item| {
            let mut args = shared.clone();
            args.insert(to.to_owned(), item.clone());
            args
        })
        .collect()
}

/// Calls `read`, on Fastmail's MCP server in `account` (or the first
/// account) for a lowercase name and on Composio otherwise, and returns
/// what it found.
async fn read_record(
    user: &User,
    read: &str,
    account: Option<&str>,
    args: Map<String, Value>,
) -> Result<Value> {
    if read.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        return user.call_fastmail(account, read, Value::Object(args)).await;
    }
    let client = user
        .composio_client()
        .ok_or_else(|| anyhow::anyhow!("Composio is not configured"))?;
    let response = client.execute_tool(read, Value::Object(args)).await?;
    if response.successful == Some(false) {
        anyhow::bail!("{read} failed");
    }
    Ok(response.data)
}

/// The IDs `args` names, such as a message ID or a list of them.
fn record_ids(args: &Value) -> Vec<&str> {
    let Value::Object(args) = args else {
        return Vec::new();
    };
    args.iter()
        .filter(|(key, _)| is_id(key))
        .flat_map(|(_, value)| match value {
            Value::String(id) => vec![id.as_str()],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        })
        .filter(|id| !id.trim().is_empty())
        .collect()
}

/// What the sender of the email `call` names is to the user. A reply or
/// forward is sent by the user, so the original's sender is who they answer.
fn sender_label(call: &ToolCall) -> &'static str {
    match ToolSlug::new(&call.name).as_str() {
        "GMAIL_REPLY_TO_THREAD" | "OUTLOOK_REPLY_EMAIL" => "Replying to",
        "FASTMAIL_SEND_EMAIL" => match call.arguments.get("mode").and_then(Value::as_str) {
            Some("forward") => "Forwarding from",
            _ => "Replying to",
        },
        _ => "From",
    }
}

/// The fields of `record` that tell the user which one it is.
fn record_fields(record: &Value, call: &ToolCall) -> Vec<ApprovalField> {
    let mut labels = HashSet::new();
    let mut shown: Vec<ApprovalField> = RECORD_KEYS
        .iter()
        .filter_map(|(key, label)| {
            let value = record_value(record, key)?;
            let label = if *label == "From" {
                sender_label(call)
            } else {
                label
            };
            labels.insert(label).then(|| ApprovalField {
                label: label.to_owned(),
                value: value.chars().take(MAX_VALUE_CHARS).collect(),
            })
        })
        .collect();
    if !labels.contains("Title")
        && let Some(title) = notion_title(record)
    {
        shown.insert(
            0,
            ApprovalField {
                label: "Title".to_owned(),
                value: title.chars().take(MAX_VALUE_CHARS).collect(),
            },
        );
    }
    shown
}

/// A Notion page's title, kept in whichever property has the `title` type.
fn notion_title(page: &Value) -> Option<String> {
    let title = page
        .get("properties")?
        .as_object()?
        .values()
        .find(|property| property.get("type").and_then(Value::as_str) == Some("title"))?
        .get("title")?
        .as_array()?
        .iter()
        .filter_map(|part| part.get("plain_text").and_then(Value::as_str))
        .collect::<String>();
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
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
    let mut rule = rules.0.get(app.slug()).cloned().unwrap_or_default();
    rule.allowed.insert(ToolSlug::new(tool).0);
    let row = match app {
        ToolApp::Catalog(app) => row(app, rule.level, rule.allowed.into_iter().collect()),
        ToolApp::Added(slug) => {
            let mut allowed: Vec<String> = rule.allowed.into_iter().collect();
            allowed.sort();
            AppPermissionRow {
                app: slug,
                level: rule.level,
                allowed,
            }
        }
    };
    user.save_app_permissions(&row).await
}

/// The call's arguments as the user reads them.
fn fields(args: &Value) -> Vec<ApprovalField> {
    let Value::Object(args) = args else {
        return Vec::new();
    };
    args.iter()
        .filter_map(|(key, value)| {
            let value = match value {
                // A list of IDs means nothing to the user, but how many it
                // names does, as when a batch trashes hundreds of emails.
                Value::Array(items) if is_id(key) && items.len() > 1 => items.len().to_string(),
                _ if is_id(key) => return None,
                Value::Null => return None,
                Value::String(text) => text.trim().to_owned(),
                Value::Array(items) if items.iter().all(|item| address(item).is_some()) => items
                    .iter()
                    .filter_map(address)
                    .collect::<Vec<_>>()
                    .join(", "),
                Value::Array(items) if items.iter().all(Value::is_string) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            if value.is_empty() || value == "[]" || value == "{}" {
                return None;
            }
            Some(ApprovalField {
                label: field_label(key),
                value: shorten(&value, MAX_VALUE_CHARS),
            })
        })
        .take(MAX_FIELDS)
        .collect()
}

/// `text` cut to `max` characters, with "..." when something was cut.
pub(crate) fn shorten(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
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

/// An argument's name as the user reads it: `recipient_email` becomes
/// "Recipient email", and a list of IDs is named by what it lists, so
/// `addLabelIds` becomes "Add labels".
pub(crate) fn field_label(key: &str) -> String {
    let mut words = String::new();
    for c in key.chars() {
        if c == '_' {
            words.push(' ');
        } else if c.is_uppercase() {
            words.push(' ');
            words.extend(c.to_lowercase());
        } else {
            words.push(c);
        }
    }
    let words = words.trim();
    let words = match words.strip_suffix(" ids") {
        Some(listed) => format!("{listed}s"),
        None if words == "ids" => "items".to_owned(),
        None => words.to_owned(),
    };
    capitalize(&words)
}

/// An argument that only names a record, such as `message_id` or `emailId`,
/// whose value means nothing to the user. Label IDs are names like `TRASH`
/// or `INBOX`, so they are shown.
pub(crate) fn is_id(key: &str) -> bool {
    if key.to_ascii_lowercase().contains("label") {
        return false;
    }
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
        match check(&self.shared.rules, &self.app, self.slug.as_str(), &ctx.args) {
            Check::Run => self.tool.execute(ctx).await,
            Check::Refuse(reason) => Ok(ToolResult::success(ctx.tool_call_id, reason)),
            Check::Ask(_) if self.shared.approved.0.as_deref() == Some(ctx.tool_call_id) => {
                self.tool.execute(ctx).await
            }
            Check::Ask(approval) => {
                if let Some(grants) = &self.shared.grants
                    && let Some(grant) = grants
                        .claim(self.slug.as_str(), &ctx.args, ctx.tool_call_id)
                        .await
                {
                    let args = ctx.args.clone();
                    let result = self.tool.execute(ctx).await?;
                    if result.is_success() && went_through(&result.output()) {
                        grants.report(&grant, self.slug.as_str(), &args);
                    }
                    return Ok(result);
                }
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
                .then(|| describe_action(&app, call, conversation))
        })
        .collect()
}

/// Whether a tool result says the call did what it was asked.
pub(crate) fn went_through(output: &str) -> bool {
    if output == DENIED || output == SKIPPED || output.starts_with(READ_ONLY) {
        return false;
    }
    let failed = parse_output(output).is_some_and(|result| {
        result.get("successful") == Some(&Value::Bool(false))
            || result.get("error").is_some_and(|error| !error.is_null())
    });
    !failed
}

fn describe_action(app: &ToolApp, call: &ToolCall, conversation: &Conversation) -> String {
    let action = action_name(app, ToolSlug::new(&call.name).as_str());
    let mut fields = records(call, conversation);
    fields.extend(fields_of(&call.arguments));
    let details: Vec<String> = fields
        .into_iter()
        .take(MAX_ACTION_FIELDS)
        .map(|field| {
            let value = field.value.split_whitespace().collect::<Vec<_>>().join(" ");
            format!(
                "{}: {}",
                field.label,
                shorten(&value, MAX_ACTION_VALUE_CHARS)
            )
        })
        .collect();
    if details.is_empty() {
        format!("{}: {action}", app.name())
    } else {
        format!("{}: {action} ({})", app.name(), details.join("; "))
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
        assert_eq!((app.slug(), acts), ("gmail", Acts::Always));
        assert_eq!(lookup("GMAIL_FETCH_EMAILS").unwrap().1, Acts::Never);
        let (app, acts) = lookup("FASTMAIL_UPDATE_EMAIL").unwrap();
        assert_eq!((app.slug(), acts), ("fastmail", Acts::WhenMoving));
        assert!(lookup("open_maps").is_none());
        // A tool of a server the user added always asks.
        let (app, acts) = lookup("MCP_MY-NOTES_SEARCH_PAGES").unwrap();
        assert_eq!((app.slug(), acts), ("mcp_my-notes", Acts::Always));
        assert_eq!(app.name(), "My notes");
        assert_eq!(
            action_name(&app, "MCP_MY-NOTES_SEARCH_PAGES"),
            "search pages"
        );
        assert!(call_acts("MCP_MY-NOTES_SEARCH_PAGES", &Value::Null).is_none());
        assert!(lookup("MCP_").is_none());
    }

    #[test]
    fn only_tools_that_never_act_are_read_tools() {
        let none = HashSet::new();
        assert_eq!(read_tool_app("gmail_fetch_emails", &none), Some("gmail"));
        assert_eq!(read_tool_app("GMAIL_SEND_EMAIL", &none), None);
        assert_eq!(read_tool_app("FASTMAIL_UPDATE_EMAIL", &none), None);
        assert_eq!(read_tool_app("MCP_MY-NOTES_SEARCH_PAGES", &none), None);
        assert_eq!(read_tool_app("open_maps", &none), None);
        let hinted = HashSet::from([ToolSlug::new("GMAIL_FETCH_EMAILS")]);
        assert_eq!(read_tool_app("GMAIL_FETCH_EMAILS", &hinted), None);
    }

    /// A tool that is only its name.
    struct Named(&'static str);

    #[async_trait]
    impl Tool for Named {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            ""
        }
        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }
        async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success(ctx.tool_call_id, ""))
        }
    }

    #[test]
    fn an_untrusted_turn_keeps_only_read_tools() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(Named("GMAIL_FETCH_EMAILS")),
            Arc::new(Named("GMAIL_SEND_EMAIL")),
            Arc::new(Named("FASTMAIL_UPDATE_EMAIL")),
            Arc::new(Named("MCP_MY-NOTES_SEARCH_PAGES")),
        ];
        let kept: Vec<String> = read_tools_only(tools, &HashSet::new())
            .iter()
            .map(|tool| tool.name().to_owned())
            .collect();
        assert_eq!(kept, ["GMAIL_FETCH_EMAILS"]);
        assert!(crate::wake::Rule::Accounts.untrusted());
        assert!(crate::wake::Rule::Digest.untrusted());
        assert!(crate::wake::Rule::Triaged.untrusted());
        assert!(!crate::wake::Rule::Place.untrusted());
    }

    #[test]
    fn lowercase_composio_names_are_classified() {
        let none = HashSet::new();
        let (app, acts) = classify("outlook_reply_email", &none).unwrap();
        assert_eq!((app.slug(), acts), ("outlook", Acts::Always));
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
        // A read the catalog checked stays a read whatever its tags say.
        let hinted: HashSet<ToolSlug> = [ToolSlug::new("SLACK_OPEN_DM")].into();
        assert_eq!(classify("slack_open_dm", &hinted).unwrap().1, Acts::Never);
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
                ("Move to trash", false),
                ("Batch modify messages", false)
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
    fn closing_open_calls_gives_each_a_result() {
        let call = |id: &str| ToolCall {
            id: id.into(),
            name: "GMAIL_SEND_EMAIL".into(),
            arguments: json!({}),
        };
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user("hi"));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![call("a"), call("b")],
            None,
        ));
        conversation.add(ConversationEntry::tool("a", "sent"));
        close_open_calls(&mut conversation, SUPERSEDED);
        assert!(open_calls(&conversation).is_empty());
        // Sanitizing for the model keeps the call now that it has a result.
        conversation.sanitize();
        let kept = conversation.entries().iter().any(|entry| {
            matches!(&entry.kind, EntryKind::Tool { call_id, output } if call_id == "b" && output == SUPERSEDED)
        });
        assert!(kept);
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
    fn a_fastmail_email_names_itself() {
        let email = json!({
            "id": "M1", "subject": "Your order shipped",
            "from": [{"name": "Shop", "email": "orders@example.com"}],
        });
        assert_eq!(record_ids(&json!({"ids": ["M1", ""], "keep": "x"})), ["M1"]);
        let delete = ToolCall {
            id: "delete".into(),
            name: "FASTMAIL_DELETE_EMAIL".into(),
            arguments: json!({"ids": ["M1"]}),
        };
        let shown: Vec<_> = record_fields(find_record(&email, "M1").unwrap(), &delete)
            .into_iter()
            .map(|f| (f.label, f.value))
            .collect();
        assert_eq!(
            shown,
            [
                ("Subject".into(), "Your order shipped".into()),
                ("From".into(), "Shop <orders@example.com>".into())
            ]
        );
    }

    #[test]
    fn a_reply_names_who_it_answers() {
        let email = json!({
            "id": "M1", "subject": "Booking",
            "from": [{"name": "Hotel", "email": "desk@example.com"}],
        });
        let send = |mode| ToolCall {
            id: "send".into(),
            name: "FASTMAIL_SEND_EMAIL".into(),
            arguments: json!({"mode": mode, "emailId": "M1", "body": "Thanks"}),
        };
        let sender = |call| record_fields(&email, &call).pop().unwrap();
        assert_eq!(sender(send("reply")).label, "Replying to");
        assert_eq!(sender(send("forward")).label, "Forwarding from");
    }

    #[test]
    fn a_read_takes_the_ids_a_call_names() {
        let lookup = |tool| LOOKUPS.iter().find(|l| l.tools.contains(&tool)).unwrap();
        let reads = read_args(
            lookup("FASTMAIL_DELETE_EMAIL"),
            &json!({"ids": ["M1", "", "M2"]}),
        );
        assert_eq!(
            reads,
            [json!({"id": "M1"}), json!({"id": "M2"})].map(|v| v.as_object().unwrap().clone())
        );
        let reads = read_args(
            lookup("GOOGLECALENDAR_DELETE_EVENT"),
            &json!({"event_id": "e1", "calendar_id": "primary", "send_updates": "all"}),
        );
        assert_eq!(
            Value::Object(reads[0].clone()),
            json!({"event_id": "e1", "calendar_id": "primary"})
        );
        let reads = read_args(
            lookup("GITHUB_CREATE_AN_ISSUE_COMMENT"),
            &json!({"owner": "o", "repo": "r", "issue_number": 7, "body": "hi"}),
        );
        assert_eq!(
            Value::Object(reads[0].clone()),
            json!({"owner": "o", "repo": "r", "issue_number": 7})
        );
        assert!(read_args(lookup("TODOIST_CLOSE_TASK"), &json!({})).is_empty());
        // Every tool listed is one that asks, so a renamed slug fails here.
        for tool in LOOKUPS.iter().flat_map(|l| l.tools) {
            let acts = call_acts(tool, &json!({"folder": "x"})).is_some_and(|(_, acts)| acts);
            assert!(acts, "{tool} is not an acting tool");
        }
    }

    #[test]
    fn a_notion_page_names_itself_by_its_title_property() {
        let page = json!({"id": "p1", "properties": {
            "Status": {"type": "select"},
            "Name": {"type": "title", "title": [{"plain_text": "Trip "}, {"plain_text": "plan"}]},
        }});
        let update = ToolCall {
            id: "update".into(),
            name: "NOTION_UPDATE_PAGE".into(),
            arguments: json!({"page_id": "p1"}),
        };
        let shown = record_fields(&page, &update);
        assert_eq!(
            (shown[0].label.as_str(), shown[0].value.as_str()),
            ("Title", "Trip plan")
        );
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
    fn values_at_the_limit_are_not_marked_cut() {
        let exact = "a".repeat(MAX_VALUE_CHARS);
        assert_eq!(fields(&json!({ "body": exact }))[0].value, exact);
        let long = "é".repeat(MAX_VALUE_CHARS + 1);
        let shown = &fields(&json!({ "body": long }))[0].value;
        assert_eq!(shown.chars().count(), MAX_VALUE_CHARS + 3);
        assert!(shown.ends_with("é..."));
        let gmail = catalog_app("gmail").expect("gmail is in the catalog");
        let call = ToolCall {
            id: "send".into(),
            name: "GMAIL_SEND_EMAIL".into(),
            arguments: json!({ "subject": "s".repeat(MAX_ACTION_VALUE_CHARS) }),
        };
        let action = describe_action(&ToolApp::Catalog(gmail), &call, &Conversation::new());
        assert!(!action.contains("..."), "{action}");
    }

    #[test]
    fn a_batch_shows_its_labels_and_how_many_it_names() {
        let shown = fields(&json!({
            "messageIds": ["M1", "M2", "M3"],
            "addLabelIds": ["TRASH"],
            "removeLabelIds": [],
        }));
        let shown: Vec<(&str, &str)> = shown
            .iter()
            .map(|field| (field.label.as_str(), field.value.as_str()))
            .collect();
        assert_eq!(shown, [("Add labels", "TRASH"), ("Messages", "3")]);
        assert!(record_ids(&json!({"addLabelIds": ["TRASH"], "messageIds": ["M1"]})) == ["M1"]);
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
                call("read_only", "GMAIL_SEND_EMAIL", email.clone()),
                call("failed", "GMAIL_SEND_EMAIL", email),
            ],
            None,
        ));
        conversation.add(ConversationEntry::tool("read", "[]"));
        conversation.add(ConversationEntry::tool("sent", r#"{"successful": true}"#));
        conversation.add(ConversationEntry::tool("denied", DENIED));
        conversation.add(ConversationEntry::tool(
            "read_only",
            format!("{READ_ONLY} Gmail to read only."),
        ));
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
