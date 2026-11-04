//! Wraps acting tools so a call asks first, runs or is refused by the app's level.

use super::Approved;
use super::Rules;
use super::ToolApp;
use super::ToolSlug;
use super::action_name;
use super::approval_for;
use super::call_fields;
use super::lookup;
use super::lookup_in;
use super::records;
use super::row;
use super::went_through;
use crate::apps::Acts;
use crate::{
    account::user::User,
    apps::grants::RoutineGrants,
    protocol::{Approval, HistoryEntry, PermissionLevel},
    store::{AppPermissionRow, ChatAllow},
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::{
    core::{Conversation, ConversationEntry, EntryKind},
    types::{
        ToolCall,
        tool::{InterruptPayload, Tool, ToolContext, ToolResult},
    },
};
use serde_json::Value;
use std::{any::Any, collections::HashSet, sync::Arc};
use uuid::Uuid;

pub(super) struct Shared {
    rules: Rules,
    approved: Approved,
    /// The grants of the routine whose run this is, tried before asking.
    grants: Option<Arc<RoutineGrants>>,
}

/// The app of `tool` and whether it acts; Composio tags only add asking, unless `over_hints`.
pub(super) fn classify(tool: &str, hinted: &HashSet<ToolSlug>) -> Option<(ToolApp, Acts)> {
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

/// The catalog app of `tool` when no call of it can act.
pub(crate) fn read_tool_app(tool: &str, hinted: &HashSet<ToolSlug>) -> Option<&'static str> {
    match classify(tool, hinted)? {
        (ToolApp::Catalog(app), Acts::Never) => Some(app.slug),
        _ => None,
    }
}

/// Only the `tools` that never act, for a turn on untrusted text.
pub(crate) fn read_tools_only(
    tools: Vec<Arc<dyn Tool>>,
    hinted: &HashSet<ToolSlug>,
) -> Vec<Arc<dyn Tool>> {
    tools
        .into_iter()
        .filter(|tool| read_tool_app(tool.name(), hinted).is_some())
        .collect()
}

/// Puts the app tools under the user's rules; in a routine's run, `grants` may let an asking call through.
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
pub(super) const ASKS_FIRST: &str = "Beep shows the user this call, with its full arguments, \
     and it runs only once they allow it.";

pub(super) struct Guarded {
    tool: Arc<dyn Tool>,
    slug: ToolSlug,
    description: String,
    app: ToolApp,
    acts: Acts,
    shared: Arc<Shared>,
}

/// What a guarded call stops the run with; the session parks on it until the user answers.
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

/// The result for a call left unanswered when a routine's turn took its card away.
pub(crate) const SUPERSEDED: &str = "Not done: a routine ran in this chat before the user \
     answered, and the question was dropped. Do not retry unless they ask.";

/// How the result of a call refused under read only starts.
pub(super) const READ_ONLY: &str = "Not done: the user set";

/// The last model turn's calls without a result: the one asking and those after it.
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

/// Records `output` as the result of every open call.
pub(crate) fn close_open_calls(conversation: &mut Conversation, output: &str) {
    for call in open_calls(conversation) {
        conversation.add(ConversationEntry::tool(&call.id, output));
    }
}

/// The card for the first open call, the one an answer resumes.
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

/// Stops asking for `tool` in its app.
pub(crate) async fn allow_always(user: &User, tool: &str) -> Result<()> {
    allow_always_in(user, tool, false).await
}

/// [`allow_always`] in a chat that may be a project's.
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

/// Stops asking for `tool` in `chat` only ("Allow for this session").
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
