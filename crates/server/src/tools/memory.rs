use super::node;
use crate::{
    memories::hit,
    protocol::MemorySource,
    store::{Fact, MEMORY_CATEGORIES, NodeRow, Origin, Scope, Validity},
    user::User,
};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::sync::Arc;

/// Stores and updates in one chat, which becomes the origin of what they write.
struct MemoryWriter {
    user: Arc<User>,
    session_id: String,
}

impl MemoryWriter {
    fn origin(&self) -> Origin {
        Origin {
            reference: self.session_id.clone(),
            quote: None,
        }
    }
}

struct MemoryStoreTool(MemoryWriter);
struct MemoryRecallTool(Arc<User>);
struct MemoryForgetTool(Arc<User>);
struct MemoryUpdateTool(MemoryWriter);
struct MemoryHistoryTool(Arc<User>);

/// Registers the memory tools on one user's memories, in the chat `session_id`.
pub fn register(registry: &mut ToolRegistry, user: &Arc<User>, session_id: &str) {
    let writer = || MemoryWriter {
        user: user.clone(),
        session_id: session_id.to_owned(),
    };
    registry.register(Arc::new(MemoryStoreTool(writer())));
    registry.register(Arc::new(MemoryRecallTool(user.clone())));
    registry.register(Arc::new(MemoryForgetTool(user.clone())));
    registry.register(Arc::new(MemoryUpdateTool(writer())));
    registry.register(Arc::new(MemoryHistoryTool(user.clone())));
}

/// `memory_recall` alone, for agents that only read memories.
pub(crate) fn recall_tool(user: &Arc<User>) -> Arc<dyn Tool> {
    Arc::new(MemoryRecallTool(user.clone()))
}

const SCOPE_HELP: &str = "What the fact is about: `user` (the default) for the user and their life, \
    `machine:<name>` for one of their machines by its name in `node list`, `project:<name>` for a \
    project such as a repository, `account:<app>` for one of their accounts, such as `account:github`.";

/// Scopes as the model writes them, `machine:build-box`. The store keeps a
/// machine by its key, so a renamed machine keeps its memories.
struct ScopeNames(Vec<NodeRow>);

impl ScopeNames {
    async fn load(user: &User) -> Result<Self> {
        Ok(Self(user.nodes().await?))
    }

    fn parse(&self, text: &str) -> Result<Scope> {
        let text = text.trim();
        if text.is_empty() || text == "user" {
            return Ok(Scope::User);
        }
        let Some((kind, name)) = text.split_once(':') else {
            bail!("unknown scope {text:?}. {SCOPE_HELP}");
        };
        match kind {
            "machine" => {
                let Some(node) = node::find(&self.0, name) else {
                    bail!("the user has no machine called {name:?}; `node list` shows them");
                };
                Ok(Scope::Machine(
                    node.endpoint.parse().context("a stored machine key")?,
                ))
            }
            "project" => Scope::project(name),
            "account" => Scope::account(name),
            _ => bail!("unknown scope {text:?}. {SCOPE_HELP}"),
        }
    }

    /// How the model reads `scope`, `None` for the user.
    fn name(&self, scope: &Scope) -> Option<String> {
        Some(match scope {
            Scope::User => return None,
            Scope::Machine(key) => {
                let key = key.to_string();
                match self.0.iter().find(|node| node.endpoint == key) {
                    Some(node) => format!("machine:{}", node.name),
                    None => "machine:(unlinked)".to_owned(),
                }
            }
            Scope::Project(name) => format!("project:{name}"),
            Scope::Account(app) => format!("account:{app}"),
        })
    }

    /// The `scope` argument, when the call gave one.
    fn arg(&self, args: &Value) -> Result<Option<Scope>> {
        opt_arg(args, "scope")
            .map(|text| self.parse(text))
            .transpose()
    }
}

pub(super) fn arg<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    opt_arg(args, name).ok_or_else(|| anyhow!("missing string argument: {name}"))
}

pub(super) fn opt_arg<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str)
}

fn date(args: &Value, name: &str) -> Result<Option<NaiveDate>> {
    opt_arg(args, name)
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| anyhow!("{name} must be YYYY-MM-DD"))
}

/// The fact to store or update, from the shared argument names.
pub(super) fn fact(args: &Value, source: MemorySource, origin: Option<Origin>) -> Result<Fact<'_>> {
    let valid = Validity {
        from: date(args, "valid_from")?,
        until: date(args, "valid_until")?,
    };
    Ok(Fact {
        content: arg(args, "content")?,
        category: None,
        source,
        valid: (valid != Validity::default()).then_some(valid),
        origin,
        scope: None,
    })
}

pub(super) fn fact_schema(required: &[&str], extra: Value) -> Value {
    let mut properties = json!({
        "content":{"type":"string","description":"One short self-contained sentence."},
        "valid_from":{"type":"string","description":"YYYY-MM-DD the fact starts holding, for temporary facts like a trip."},
        "valid_until":{"type":"string","description":"YYYY-MM-DD of the last day the fact holds. Leave out for lasting facts."}
    });
    if let (Some(properties), Some(extra)) = (properties.as_object_mut(), extra.as_object()) {
        properties.extend(extra.clone());
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn reply(ctx: &ToolContext<'_>, output: Value) -> Result<ToolResult> {
    Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
}

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &str {
        "memory_store"
    }
    fn description(&self) -> &str {
        "Remember a fact as one short self-contained sentence that names its subject: about the user, or about one of their machines, projects or accounts, given as scope. A fact that says the same as an existing memory in its scope replaces it. Give valid_from/valid_until for facts that only hold for a while, like a trip or an appointment. Never store secrets."
    }
    fn parameters(&self) -> Value {
        fact_schema(
            &["content"],
            json!({"scope":{"type":"string","description":SCOPE_HELP}}),
        )
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let names = ScopeNames::load(&self.0.user).await?;
        let mut fact = fact(&ctx.args, MemorySource::User {}, Some(self.0.origin()))?;
        fact.scope = names.arg(&ctx.args)?;
        let content = fact.content;
        let remembered = self.0.user.remember(fact).await?;
        let change = if remembered.replaced {
            "updated"
        } else {
            "stored"
        };
        // The content lets the chat show what was remembered.
        reply(
            &ctx,
            json!({change: true, "id": remembered.id, "content": content}),
        )
    }
}

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &str {
        "memory_recall"
    }
    fn description(&self) -> &str {
        "Search the user's memories by meaning, best first. An empty query lists the most recent. Each result has an id, a source (user, learned from a chat, email or slack), its last change date, for temporary facts its days, when known a quote of the words it came from, and a scope when it is about a machine, project or account rather than the user."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string"},
            "category":{"type":"string","enum":MEMORY_CATEGORIES,"description":"Puts memories in this category first; the others follow."},
            "scope":{"type":"string","description":"Only memories in this scope, such as `machine:build-box` or `project:beep`. Leave out to search them all."}
        },"required":["query"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let query = arg(&ctx.args, "query")?;
        // An empty query is a request to see what Beep knows.
        let limit = if query.trim().is_empty() { 100 } else { 30 };
        let category = opt_arg(&ctx.args, "category");
        let names = ScopeNames::load(&self.0).await?;
        let scope = names.arg(&ctx.args)?;
        let scope = scope.as_ref();
        let mut hits = self
            .0
            .search_memories(query, category, scope, limit)
            .await?;
        // A model can guess a category the memory was not filed under, such
        // as "place" for a hotel stay filed under "plan", so the rest follow.
        if category.is_some() && hits.len() < limit {
            for hit in self.0.search_memories(query, None, scope, limit).await? {
                if hits.len() == limit {
                    break;
                }
                if !hits.iter().any(|h| h.id == hit.id) {
                    hits.push(hit);
                }
            }
        }
        reply(
            &ctx,
            hits.iter()
                .map(|h| h.to_json(names.name(&h.scope).as_deref()))
                .collect(),
        )
    }
}

#[async_trait]
impl Tool for MemoryForgetTool {
    fn name(&self) -> &str {
        "memory_forget"
    }
    fn description(&self) -> &str {
        "Forget a memory by the id memory_recall gave."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let forgotten = self.0.forget(arg(&ctx.args, "id")?).await?;
        reply(&ctx, json!({"forgotten": forgotten}))
    }
}

#[async_trait]
impl Tool for MemoryUpdateTool {
    fn name(&self) -> &str {
        "memory_update"
    }
    fn description(&self) -> &str {
        "Replace a memory's text by the id memory_recall gave, with the full new sentence. Days and scope stay unless given."
    }
    fn parameters(&self) -> Value {
        fact_schema(
            &["id", "content"],
            json!({"id":{"type":"string"},"scope":{"type":"string","description":SCOPE_HELP}}),
        )
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let names = ScopeNames::load(&self.0.user).await?;
        let mut fact = fact(&ctx.args, MemorySource::User {}, Some(self.0.origin()))?;
        fact.scope = names.arg(&ctx.args)?;
        let content = fact.content;
        let updated = self
            .0
            .user
            .update_memory(arg(&ctx.args, "id")?, fact)
            .await?;
        reply(&ctx, json!({"updated": updated, "content": content}))
    }
}

#[async_trait]
impl Tool for MemoryHistoryTool {
    fn name(&self) -> &str {
        "memory_history"
    }
    fn description(&self) -> &str {
        "Earlier versions of a memory by the id memory_recall gave, newest first, each with when it was replaced, for questions like when something changed. Kept for a few months."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let history = self.0.memory_history(arg(&ctx.args, "id")?).await?;
        reply(
            &ctx,
            history
                .into_iter()
                .map(|old| {
                    let mut value = hit(old.row).to_json(None);
                    value["replaced"] = json!(old.superseded_at);
                    value
                })
                .collect(),
        )
    }
}
