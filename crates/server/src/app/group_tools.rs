//! What Beep can reach from a group beyond the chat: the web, and the
//! members' apps. Each app tool takes a `member` argument naming whose
//! account the call runs as. Reading runs straight away; anything that acts
//! asks that member on their phone and runs once they allow it.
//!
//! The run holds members' private memories and other members' mail, and
//! the leak gate only reads the reply. So text a call would send out, a URL
//! or an email, goes through the same gate first, and what another member's
//! app returns joins the secrets the reply is checked against.

use super::{App, UserNotice, group::said, run::web_tools};
use crate::{
    apps::{CatalogApp, markdown},
    decision::DecisionTool,
    leak::{Gate, Said, Secret},
    memories::clip,
    permissions::{self, Rules, ToolApp, ToolSlug},
    protocol::{Approval, Notice, PendingApproval},
    tools::group_memory::{Member, Secrets},
    user::User,
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::{
    core::CancellationToken,
    types::{
        ToolRegistry,
        tool::{Tool, ToolContext, ToolResult},
    },
};
use futures::future::join_all;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, PoisonError},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// The app tools a group may use. Mail and files are read only; creating an
/// event, a task or a page and sending an email ask the owner first.
const GROUP_TOOLS: &[&str] = &[
    "GMAIL_FETCH_EMAILS",
    "GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID",
    "GMAIL_SEARCH_PEOPLE",
    "GMAIL_SEND_EMAIL",
    "GOOGLECALENDAR_EVENTS_LIST_ALL_CALENDARS",
    "GOOGLECALENDAR_FREE_BUSY_QUERY",
    "GOOGLECALENDAR_CREATE_EVENT",
    "OUTLOOK_SEARCH_MESSAGES",
    "OUTLOOK_GET_MESSAGE",
    "OUTLOOK_QUERY_EMAILS",
    "OUTLOOK_SEND_EMAIL",
    "OUTLOOK_GET_CALENDAR_VIEW",
    "OUTLOOK_CALENDAR_CREATE_EVENT",
    "GOOGLEDRIVE_LIST_FILES",
    "GOOGLEDRIVE_GET_FILE_METADATA",
    "GOOGLEDRIVE_EXPORT_GOOGLE_WORKSPACE_FILE",
    "ONE_DRIVE_SEARCH_DRIVE_ITEMS",
    "ONE_DRIVE_GET_ITEM",
    "NOTION_SEARCH_NOTION_PAGE",
    "NOTION_GET_PAGE_MARKDOWN",
    "NOTION_QUERY_DATABASE",
    "NOTION_CREATE_NOTION_PAGE",
    "TODOIST_FILTER_TASKS",
    "TODOIST_GET_ALL_TASKS",
    "TODOIST_GET_ALL_PROJECTS",
    "TODOIST_CREATE_TASK",
    "FASTMAIL_SEARCH_EMAIL",
    "FASTMAIL_READ_EMAIL",
    "FASTMAIL_READ_THREAD",
    "FASTMAIL_SEND_EMAIL",
    "FASTMAIL_LIST_CALENDARS",
    "FASTMAIL_SEARCH_EVENTS",
    "FASTMAIL_CREATE_EVENT",
];

/// Most of another member's app result the leak gate keeps.
const SECRET_CHARS: usize = 4000;

/// How long an action waits for its owner's answer.
const ACTION_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// What a call gets back when its arguments would reveal a memory.
const LEAKS: &str = "Not done: this would send out a member's private memories. \
Leave them out and try again, or do without.";

/// One group run, as its tools need it.
pub(super) struct GroupRun {
    pub app: Arc<App>,
    pub group_id: Uuid,
    pub group_name: String,
    /// Who sent the message Beep is answering.
    pub actor: Member,
    pub secrets: Secrets,
    pub chat: Vec<Said>,
    pub decision: Arc<DecisionTool>,
}

impl GroupRun {
    /// Whether `text`, about to leave the run in a tool call, reveals a
    /// member's memory.
    pub(super) async fn leaks(&self, text: &str) -> bool {
        let secrets = self
            .secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let gate = Gate {
            decision: &self.decision,
            secrets: &secrets,
            chat: &self.chat,
        };
        gate.check(text).await.is_some()
    }
}

/// An action waiting for its owner's OK.
pub(super) struct GroupAction {
    owner: String,
    owner_name: String,
    group_id: Uuid,
    app: &'static str,
    action: String,
    tool: Arc<dyn Tool>,
    args: Value,
    asked: Instant,
}

/// Registers the web tools and the apps of `members`, all screened.
pub(super) async fn register(
    registry: &mut ToolRegistry,
    run: Arc<GroupRun>,
    members: Vec<Member>,
    firecrawl_key: Option<&str>,
) {
    for tool in firecrawl_key.map(web_tools).unwrap_or_default() {
        registry.register(Arc::new(Screened {
            tool,
            run: run.clone(),
        }));
    }
    /// A member Beep may use the apps of in this run.
    struct AppsOf {
        member: Member,
        /// The apps each tool of theirs belongs to, and what they allow there.
        rules: Rules,
        tools: Vec<Arc<dyn Tool>>,
    }

    let loaded = join_all(members.into_iter().map(|member| async move {
        let user = member.user.clone();
        let (composio, mcp, rules) =
            tokio::join!(user.app_tools(), user.mcp_tools(), user.app_rules());
        let rules = rules.unwrap_or_else(|error| {
            log::warn!("permissions: unable to load, asking for everything: {error:#}");
            Rules::default()
        });
        let tools = markdown::wrap(composio.tools.into_iter().chain(mcp).collect());
        AppsOf {
            member,
            rules,
            tools,
        }
    }))
    .await;

    let mut by_name: BTreeMap<String, Vec<Owned>> = BTreeMap::new();
    for apps in loaded {
        let member = Arc::new(apps.member);
        for tool in apps.tools {
            let slug = ToolSlug::new(tool.name());
            if !GROUP_TOOLS.contains(&slug.as_str()) {
                continue;
            }
            let Some((app, acts)) = permissions::call_acts(tool.name(), &Value::Null) else {
                continue;
            };
            if acts && apps.rules.read_only(app.slug) {
                continue;
            }
            by_name
                .entry(slug.as_str().to_owned())
                .or_default()
                .push(Owned {
                    member: member.clone(),
                    tool,
                    app,
                });
        }
    }
    for owners in by_name.into_values() {
        registry.register(Arc::new(MemberTool::new(owners, run.clone())));
    }
}

/// A web tool whose arguments go through the leak gate before it runs.
struct Screened {
    tool: Arc<dyn Tool>,
    run: Arc<GroupRun>,
}

#[async_trait]
impl Tool for Screened {
    fn name(&self) -> &str {
        self.tool.name()
    }
    fn description(&self) -> &str {
        self.tool.description()
    }
    fn parameters(&self) -> Value {
        self.tool.parameters()
    }
    fn timeout_secs(&self) -> Option<u64> {
        self.tool.timeout_secs()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        if self.run.leaks(&ctx.args.to_string()).await {
            return Ok(ToolResult::failure(ctx.tool_call_id, LEAKS));
        }
        self.tool.execute(ctx).await
    }
}

/// One member's copy of an app tool.
struct Owned {
    member: Arc<Member>,
    tool: Arc<dyn Tool>,
    app: &'static CatalogApp,
}

/// An app tool every member who has it can be named in.
struct MemberTool {
    name: String,
    description: String,
    parameters: Value,
    owners: Vec<Owned>,
    run: Arc<GroupRun>,
}

impl MemberTool {
    fn new(owners: Vec<Owned>, run: Arc<GroupRun>) -> Self {
        let first = &owners[0];
        let names: Vec<&str> = owners.iter().map(|o| o.member.name.as_str()).collect();
        let acts =
            permissions::call_acts(first.tool.name(), &Value::Null).is_some_and(|(_, acts)| acts);
        let mut description = format!(
            "{} Runs on the account of the member named in `member`: {}.",
            first.tool.description(),
            names.join(", ")
        );
        if acts {
            description.push_str(
                " It asks that member on their phone first and runs once they allow it; \
                 Beep tells the group then.",
            );
        }
        Self {
            name: first.tool.name().to_owned(),
            description,
            parameters: with_member(first.tool.parameters(), &names),
            owners,
            run,
        }
    }
}

/// `schema` with a required `member` naming one of `names`.
fn with_member(mut schema: Value, names: &[&str]) -> Value {
    if !schema.is_object() {
        schema = json!({"type": "object"});
    }
    let member = json!({
        "type": "string",
        "enum": names,
        "description": "The member whose account this runs on.",
    });
    match schema["properties"].as_object_mut() {
        Some(properties) => {
            properties.insert("member".to_owned(), member);
        }
        None => schema["properties"] = json!({ "member": member }),
    }
    match schema["required"].as_array_mut() {
        Some(required) => required.push(json!("member")),
        None => schema["required"] = json!(["member"]),
    }
    schema
}

#[async_trait]
impl Tool for MemberTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters(&self) -> Value {
        self.parameters.clone()
    }
    fn timeout_secs(&self) -> Option<u64> {
        self.owners[0].tool.timeout_secs()
    }
    fn max_output_bytes(&self) -> usize {
        self.owners[0].tool.max_output_bytes()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let name = ctx.args["member"].as_str().unwrap_or_default();
        let Some(owner) = self.owners.iter().find(|o| o.member.name == name) else {
            let names: Vec<&str> = self.owners.iter().map(|o| o.member.name.as_str()).collect();
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!(
                    "Name one of these members in `member`: {}.",
                    names.join(", ")
                ),
            ));
        };
        let mut args = ctx.args.clone();
        if let Some(map) = args.as_object_mut() {
            map.remove("member");
        }
        let acts = permissions::call_acts(&self.name, &args).is_some_and(|(_, acts)| acts);
        if acts {
            if self.run.leaks(&args.to_string()).await {
                return Ok(ToolResult::failure(ctx.tool_call_id, LEAKS));
            }
            let approval =
                permissions::approval_for(&ToolApp::Catalog(owner.app), &self.name, &args);
            self.run
                .app
                .ask_group_action(&self.run, owner, approval, args);
            return Ok(ToolResult::success(
                ctx.tool_call_id,
                format!(
                    "Asked {} to allow it on their phone. It runs once they do, and Beep \
                     tells the group. Say you asked; do not say it is done.",
                    owner.member.name
                ),
            ));
        }
        let result = owner
            .tool
            .execute(ToolContext::new(
                ctx.tool_call_id,
                args,
                None,
                ctx.cancel_token(),
            ))
            .await?;
        if owner.member.user.id() != self.run.actor.user.id() && result.is_success() {
            self.run
                .secrets
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Secret {
                    owner: owner.member.name.clone(),
                    content: clip(&result.output(), SECRET_CHARS),
                });
        }
        Ok(result)
    }
}

impl App {
    /// Asks `owner` on their phone to allow an action, kept until they answer.
    fn ask_group_action(&self, run: &GroupRun, owner: &Owned, approval: Approval, args: Value) {
        let query_id = Uuid::new_v4().to_string();
        let owner_id = owner.member.user.id().to_owned();
        {
            let mut actions = self
                .group_actions
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            actions.retain(|_, action| action.asked.elapsed() < ACTION_TTL);
            actions.insert(
                query_id.clone(),
                GroupAction {
                    owner: owner_id.clone(),
                    owner_name: owner.member.name.clone(),
                    group_id: run.group_id,
                    app: owner.app.name,
                    action: approval.action.clone(),
                    tool: owner.tool.clone(),
                    args,
                    asked: Instant::now(),
                },
            );
        }
        let asker = if owner_id == run.actor.user.id() {
            "You".to_owned()
        } else {
            run.actor.name.clone()
        };
        let _ = self.notifications.send(UserNotice {
            user: owner_id,
            notice: Notice {
                title: format!("Allow {}: {}?", approval.app, approval.action),
                body: format!("{asker} asked Beep in {}", run.group_name),
                session_id: run.group_id.to_string(),
                app: None,
                approval: Some(PendingApproval {
                    run_id: String::new(),
                    query_id,
                    approval,
                }),
                changed: false,
                group: true,
            },
        });
    }

    /// Runs or drops the action `query_id` that `user` was asked about, and
    /// tells the group. `false` when nothing of theirs waits under it.
    pub(crate) async fn answer_group_action(
        &self,
        user: &User,
        query_id: &str,
        allow: bool,
    ) -> Result<bool> {
        let action = {
            let mut actions = self
                .group_actions
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match actions.get(query_id) {
                Some(action) if action.owner == user.id() => actions.remove(query_id),
                _ => None,
            }
        };
        let Some(action) = action.filter(|action| action.asked.elapsed() < ACTION_TTL) else {
            return Ok(false);
        };
        let GroupAction {
            owner_name,
            group_id,
            app,
            action,
            tool,
            args,
            ..
        } = action;
        let text = if allow {
            let cancel = CancellationToken::new();
            let result = tool
                .execute(ToolContext::new(query_id, args, None, &cancel))
                .await;
            match result {
                Ok(result) if result.is_success() => {
                    format!("Done: {action} in {owner_name}'s {app}.")
                }
                Ok(result) => {
                    log::warn!("group action failed: {}; group={group_id}", result.output());
                    format!("I couldn't {action} in {owner_name}'s {app}.")
                }
                Err(error) => {
                    log::warn!("group action failed: {error:#}; group={group_id}");
                    format!("I couldn't {action} in {owner_name}'s {app}.")
                }
            }
        } else {
            format!("{owner_name} didn't allow {action} in {app}.")
        };
        self.store.post_beep(group_id, &text).await?;
        if let Some(joined) = user.group(group_id).await? {
            self.group_notice(&joined.members, &joined, None, &said("Beep", &text));
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_member_argument_is_added_and_required() {
        let schema =
            json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]});
        let schema = with_member(schema, &["Ana", "Sam"]);
        assert_eq!(
            schema["properties"]["member"]["enum"],
            json!(["Ana", "Sam"])
        );
        assert_eq!(schema["required"], json!(["q", "member"]));
        let bare = with_member(json!({}), &["Ana"]);
        assert_eq!(bare["required"], json!(["member"]));
    }

    #[test]
    fn every_group_tool_is_in_the_catalog() {
        for tool in GROUP_TOOLS {
            assert!(
                permissions::call_acts(tool, &Value::Null).is_some(),
                "{tool}"
            );
        }
    }
}
