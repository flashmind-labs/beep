//! What Beep may do in each connected app. Reading is always allowed; tools
//! that send, change or delete follow the app's level: ask first (the
//! default), always allow, or read only. Under ask first the run ends on the
//! open call, and the user's answer resumes the session from there.

use crate::{
    apps::{
        CATALOG, CatalogApp, catalog_app, custom_mcp::PREFIX as ADDED_PREFIX, mcp::FASTMAIL_TOOLS,
        read,
    },
    grants::RoutineGrants,
    protocol::{
        AppAction, AppKind, AppPermissions, Approval, ApprovalField, HistoryEntry, PermissionLevel,
    },
    store::{AppPermissionRow, ChatAllow},
    tools::media::{self, MediaKind},
    user::User,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flashmind::{
    core::{Conversation, ConversationEntry, EntryKind},
    types::{
        ToolCall,
        tool::{InterruptPayload, Tool, ToolContext, ToolResult},
    },
};
use serde_json::{Map, Value};
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;

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
    /// Only when it sends to one of the user's machines (`to` names one)
    /// rather than to the user in the chat.
    WhenToMachine,
}

impl Acts {
    fn on(self, args: &Value) -> bool {
        match self {
            Acts::Never => false,
            Acts::Always => true,
            Acts::WhenMoving => args.get("folder").is_some_and(|folder| !folder.is_null()),
            Acts::WhenToMachine => args
                .get("to")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|to| !to.is_empty() && !to.eq_ignore_ascii_case("user")),
        }
    }

    /// Whether some call of the tool can act, and so falls under the rules.
    fn can_act(self) -> bool {
        self != Acts::Never
    }
}

/// `send_file` as rules keep it.
const SEND_FILE: &str = "SEND_FILE";
/// `add_machine` as rules keep it: a setup token grants access to the
/// account, so making one acts.
const ADD_MACHINE: &str = "ADD_MACHINE";
/// `import_project` as rules keep it: it clones onto a machine with the
/// user's tokens.
const IMPORT_PROJECT: &str = "IMPORT_PROJECT";
/// The Beep MCP server's `run_on` and `send_files`, which coding agents on
/// the user's machines call (`crate::tools::machines::mcp`).
pub(crate) const RUN_ON: &str = "RUN_ON";
pub(crate) const SEND_FILES: &str = "SEND_FILES";

/// Composio tags of tools that create or destroy something. Such a tool
/// asks first even if the catalog lists it as a read.
const ACTING_HINTS: &[&str] = &["createHint", "destructiveHint"];

/// Whether a Composio tool's tags say it creates or destroys something.
pub(crate) fn hinted_to_act(tags: &[String]) -> bool {
    tags.iter().any(|tag| ACTING_HINTS.contains(&tag.as_str()))
}

/// The project agent's own tools, which ask or refuse through the project
/// gate (`crate::projects::gate`) rather than `guard`: `Acts::Never` keeps
/// `guard` and `actions_since` off them. It is outside `CATALOG`, so it is
/// not listed as an app to connect; `kind` means nothing for it. `exec` and
/// `process` are here because a project chat gates them too, so a reloaded
/// project chat shows their card; [`lookup`] leaves them out, since every
/// other chat has them as well (see [`lookup_in`]).
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

/// The project actions the user can allow always, each on its own: a merge
/// and a plan card that always asks are never among them.
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
    /// Beep's own tools that act on the user's machines, such as
    /// `send_file` sending to one.
    Machines,
    /// Beep's tools that make media with the user's own API keys, which
    /// spend their money: `image_gen`, `video_gen` and `3d_gen`.
    Media,
}

/// The rules' name for the user's machines.
pub(crate) const MACHINES_APP: &str = "machines";
/// The rules' name for media made with the user's keys.
const MEDIA_APP: &str = "media";

impl ToolApp {
    pub(crate) fn slug(&self) -> &str {
        match self {
            ToolApp::Catalog(app) => app.slug,
            ToolApp::Added(slug) => slug,
            ToolApp::Machines => MACHINES_APP,
            ToolApp::Media => MEDIA_APP,
        }
    }

    /// The app's name on an approval: "My notes" for `mcp_my-notes`.
    pub(crate) fn name(&self) -> String {
        match self {
            ToolApp::Catalog(app) => app.name.to_owned(),
            ToolApp::Added(slug) => {
                capitalize(&slug.trim_start_matches(ADDED_PREFIX).replace('-', " "))
            }
            ToolApp::Machines => "Your machines".to_owned(),
            ToolApp::Media => "Media generation".to_owned(),
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
    if tool == SEND_FILE {
        return Some((ToolApp::Machines, Acts::WhenToMachine));
    }
    if [ADD_MACHINE, IMPORT_PROJECT, RUN_ON, SEND_FILES].contains(&tool) {
        return Some((ToolApp::Machines, Acts::Always));
    }
    if MediaKind::from_tool(tool).is_some() {
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

/// [`lookup`] in a chat that is a project's, where `exec` and `process` are
/// Project's too.
pub(crate) fn lookup_in(tool: &str, is_project: bool) -> Option<(ToolApp, Acts)> {
    if is_project && let Some(found) = project_tool(ToolSlug::new(tool).as_str()) {
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

/// The catalog app of `tool` and whether a call with `args` acts, for a
/// call made outside a session, such as from a group. `None` for a tool
/// outside the catalog.
pub(crate) fn call_acts(tool: &str, args: &Value) -> Option<(&'static CatalogApp, bool)> {
    match lookup(tool)? {
        (ToolApp::Catalog(app), acts) => Some((app, acts.on(args))),
        (ToolApp::Added(_) | ToolApp::Machines | ToolApp::Media, _) => None,
    }
}

/// The card that asks to allow `tool` with `args`.
pub(crate) fn approval_for(app: &ToolApp, tool: &str, args: &Value) -> Approval {
    Approval {
        app: app.name(),
        action: action_name(app, ToolSlug::new(tool).as_str()),
        fields: call_fields(app, tool, args),
    }
}

/// The arguments a card shows. A media call shows the service and model it
/// runs on even when the model left the model out.
fn call_fields(app: &ToolApp, tool: &str, args: &Value) -> Vec<ApprovalField> {
    match (app, MediaKind::from_tool(tool)) {
        (ToolApp::Media, Some(kind)) => media::approval_fields(kind, args),
        _ => fields(args),
    }
}

/// The tools of `app` that can act, by the name the agent calls them. The
/// project's tools all count: its gate asks for them, so each can be
/// allowed always.
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
    if let (ToolApp::Media, Some(kind)) = (app, MediaKind::from_tool(tool)) {
        return kind.action().to_owned();
    }
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

    /// Adds what the user allowed for the rest of one chat: those actions
    /// run without asking under ask first. An app's level stays as it is,
    /// so read only still refuses.
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
pub(crate) enum Check {
    Run,
    /// Not done, with the result the model reads.
    Refuse(String),
    /// Asks the user first, with this card.
    Ask(Approval),
}

/// What the rules say about `tool`, of `app`, acting with `args`.
pub(crate) fn check(rules: &Rules, app: &ToolApp, tool: &str, args: &Value) -> Check {
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
/// `is_project` says the chat is a project's, whose `exec` asks as Project.
pub(crate) fn waiting_approval(
    conversation: &Conversation,
    is_project: bool,
) -> Option<HistoryEntry> {
    let call = open_calls(conversation).into_iter().next()?;
    let (app, _) = lookup_in(&call.name, is_project)?;
    let mut fields = call_fields(&app, &call.name, &call.arguments);
    fields.extend(records(&call, conversation));
    Some(HistoryEntry::Approval {
        approval: Approval {
            app: app.name(),
            action: action_name(&app, ToolSlug::new(&call.name).as_str()),
            fields,
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
        .take(MAX_RECORDS)
        .flat_map(|record| record_fields(record, call))
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
/// no tool that reads one event or contact, and Slack and Teams calls name
/// channels rather than one record.
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
        tools: &["ONE_DRIVE_MOVE_ITEM"],
        read: "ONE_DRIVE_GET_ITEM",
        args: &[("itemId", "item_id"), ("driveId", "drive_id")],
    },
    Lookup {
        tools: &["ONE_DRIVE_CREATE_LINK"],
        read: "ONE_DRIVE_GET_ITEM",
        args: &[("item_id", "item_id"), ("drive_id", "drive_id")],
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
    shown
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
    allow_always_in(user, tool, false).await
}

/// [`allow_always`] in a chat that may be a project's, where `EXEC` is
/// Project's.
pub(crate) async fn allow_always_in(user: &User, tool: &str, is_project: bool) -> Result<()> {
    let Some((app, _)) = lookup_in(tool, is_project) else {
        return Ok(());
    };
    let rules = user.app_rules().await?;
    let mut rule = rules.0.get(app.slug()).cloned().unwrap_or_default();
    rule.allowed.insert(ToolSlug::new(tool).0);
    let row = match app {
        ToolApp::Catalog(app) => row(app, rule.level, rule.allowed.into_iter().collect()),
        other => {
            let mut allowed: Vec<String> = rule.allowed.into_iter().collect();
            allowed.sort();
            AppPermissionRow {
                app: other.slug().to_owned(),
                level: rule.level,
                allowed,
            }
        }
    };
    user.save_app_permissions(&row).await
}

/// Stops asking for `tool` in `chat` only, for "Allow for this session".
/// `is_project` as for [`allow_always_in`].
pub(crate) async fn allow_in_chat(
    user: &User,
    chat: Uuid,
    tool: &str,
    is_project: bool,
) -> Result<()> {
    let Some((app, _)) = lookup_in(tool, is_project) else {
        return Ok(());
    };
    let allow = ChatAllow {
        app: app.slug().to_owned(),
        tool: ToolSlug::new(tool).0,
    };
    user.allow_in_chat(chat, &allow).await
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
            EntryKind::Tool {
                call_id, output, ..
            } => Some((calls.get(call_id.as_str())?, output)),
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
    fn a_chat_allow_runs_without_asking_but_never_past_read_only() {
        let mut rules = Rules::from_rows(vec![AppPermissionRow {
            app: "notion".into(),
            level: PermissionLevel::ReadOnly {},
            allowed: Vec::new(),
        }]);
        let args = Value::Null;
        let gmail = lookup("GMAIL_SEND_EMAIL").unwrap().0;
        assert!(matches!(
            check(&rules, &gmail, "GMAIL_SEND_EMAIL", &args),
            Check::Ask(_)
        ));
        let allow = |app: &str, tool: &str| ChatAllow {
            app: app.into(),
            tool: tool.into(),
        };
        rules.allow_in_chat(vec![
            allow("gmail", "GMAIL_SEND_EMAIL"),
            allow("notion", "NOTION_CREATE_NOTION_PAGE"),
        ]);
        assert!(matches!(
            check(&rules, &gmail, "GMAIL_SEND_EMAIL", &args),
            Check::Run
        ));
        // Only the action allowed.
        assert!(matches!(
            check(&rules, &gmail, "GMAIL_MOVE_TO_TRASH", &args),
            Check::Ask(_)
        ));
        let notion = lookup("NOTION_CREATE_NOTION_PAGE").unwrap().0;
        assert!(matches!(
            check(&rules, &notion, "NOTION_CREATE_NOTION_PAGE", &args),
            Check::Refuse(_)
        ));
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

    #[tokio::test]
    async fn sending_to_a_machine_asks_first() {
        let (app, acts) = lookup("send_file").unwrap();
        assert_eq!(
            (app.slug(), app.name()),
            (MACHINES_APP, "Your machines".into())
        );
        assert_eq!(acts, Acts::WhenToMachine);
        let guarded = guard(
            vec![Arc::new(Named("send_file"))],
            Rules::default(),
            Approved(None),
            &HashSet::new(),
            None,
        );
        let token = tokio_util::sync::CancellationToken::new();
        let run = |args: Value| {
            let tool = guarded[0].clone();
            let token = token.clone();
            async move {
                tool.execute(ToolContext::new("call", args, None, &token))
                    .await
                    .unwrap()
            }
        };
        let to_machine = run(json!({ "path": "frames", "to": "laptop" })).await;
        assert!(to_machine.is_interrupt());
        let to_user = run(json!({ "path": "a.pdf", "to": "User" })).await;
        assert!(to_user.is_success() && !to_user.is_interrupt());
        assert!(run(json!({ "path": "a.pdf" })).await.is_success());
        let approval = approval_for(&app, "send_file", &json!({ "to": "laptop" }));
        assert_eq!(approval.action, "send file");
    }

    #[tokio::test]
    async fn making_media_asks_first_with_its_service_and_model() {
        let token = tokio_util::sync::CancellationToken::new();
        let args = json!({ "prompt": "a red fox", "provider": "openai" });
        for tool in ["image_gen", "video_gen", "3d_gen"] {
            let (app, acts) = lookup(tool).unwrap();
            assert_eq!(app.slug(), MEDIA_APP);
            assert_eq!(acts, Acts::Always);
            let guarded = guard(
                vec![Arc::new(Named(tool))],
                Rules::default(),
                Approved(None),
                &HashSet::new(),
                None,
            );
            let result = guarded[0]
                .execute(ToolContext::new("call", args.clone(), None, &token))
                .await
                .unwrap();
            assert!(result.is_interrupt(), "{tool}");
            // An untrusted turn never gets it.
            assert!(read_tools_only(vec![Arc::new(Named(tool))], &HashSet::new()).is_empty());
        }
        let approval = approval_for(&ToolApp::Media, "image_gen", &args);
        assert_eq!(approval.app, "Media generation");
        assert_eq!(approval.action, "make an image");
        let shown: Vec<(&str, &str)> = approval
            .fields
            .iter()
            .map(|field| (field.label.as_str(), field.value.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("Prompt", "a red fox"),
                ("Provider", "OpenAI"),
                ("Model", "gpt-image-2")
            ]
        );
        // The user's yes runs it.
        let guarded = guard(
            vec![Arc::new(Named("image_gen"))],
            Rules::default(),
            Approved(Some("call".into())),
            &HashSet::new(),
            None,
        );
        let result = guarded[0]
            .execute(ToolContext::new("call", args, None, &token))
            .await
            .unwrap();
        assert!(!result.is_interrupt());
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
    fn project_tools_are_known_and_can_be_allowed_always() {
        let (app, acts) = lookup("project_plan").unwrap();
        assert_eq!((app.slug(), acts), ("project", Acts::Never));
        assert_eq!(app.name(), "Project");
        assert!(lookup("PROJECT_WORKER").is_some());
        assert!(lookup("PROJECT_OTHER").is_none());
        let saved = row(
            &PROJECT_APP,
            PermissionLevel::Ask {},
            vec![
                "PROJECT_PLAN".into(),
                "PROJECT_TASKS_DROP".into(),
                "PROJECT_TASKS".into(),
                "PROJECT_SETUP".into(),
                "EXEC".into(),
                "GMAIL_SEND_EMAIL".into(),
            ],
        );
        assert_eq!(
            saved.allowed,
            ["EXEC", "PROJECT_PLAN", "PROJECT_TASKS_DROP"]
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
            matches!(&entry.kind, EntryKind::Tool { call_id, output, .. } if call_id == "b" && output == SUPERSEDED)
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
        let Some(HistoryEntry::Approval { query_id, approval }) =
            waiting_approval(&conversation, false)
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
        assert!(waiting_approval(&conversation, false).is_none());
    }

    #[test]
    fn only_a_project_chat_shows_a_parked_exec_as_project() {
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user("Run the tests"));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "run".into(),
                name: "exec".into(),
                arguments: json!({"command": "cargo test"}),
            }],
            None,
        ));
        assert!(waiting_approval(&conversation, false).is_none());
        let Some(HistoryEntry::Approval { approval, .. }) = waiting_approval(&conversation, true)
        else {
            panic!("expected the project card");
        };
        assert_eq!(approval.app, "Project");
        assert!(lookup("exec").is_none());
        assert!(lookup("process").is_none());
        assert_eq!(lookup_in("exec", true).unwrap().0.slug(), "project");
        assert_eq!(
            lookup_in("project_plan", false).unwrap().0.slug(),
            "project"
        );
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
        let Some(HistoryEntry::Approval { approval, .. }) = waiting_approval(&conversation, false)
        else {
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
            lookup("ONE_DRIVE_MOVE_ITEM"),
            &json!({"itemId": "i1", "driveId": "d1", "parentReference": {"id": "p"}}),
        );
        assert_eq!(
            Value::Object(reads[0].clone()),
            json!({"item_id": "i1", "drive_id": "d1"})
        );
        assert!(read_args(lookup("NOTION_UPDATE_PAGE"), &json!({})).is_empty());
        // Every tool listed is one that asks, so a renamed slug fails here.
        for tool in LOOKUPS.iter().flat_map(|l| l.tools) {
            let acts = call_acts(tool, &json!({"folder": "x"})).is_some_and(|(_, acts)| acts);
            assert!(acts, "{tool} is not an acting tool");
        }
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
