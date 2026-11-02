use super::machines;
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

pub(crate) const NAME: &str = "memory";

/// What a `memory` call does, by its `action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Action {
    Recall,
    Store,
    Update,
    Forget,
    History,
}

impl Action {
    const READS: &[Action] = &[Action::Recall, Action::History];
    const ALL: &[Action] = &[
        Action::Recall,
        Action::Store,
        Action::Update,
        Action::Forget,
        Action::History,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        self.into()
    }
}

/// `memory`: one tool for the user's memories. Without a writer it only
/// recalls and reads history, for agents that must not change memories.
struct MemoryTool {
    user: Arc<User>,
    writer: Option<MemoryWriter>,
}

/// Registers `memory` on one user's memories, in the chat `session_id`.
pub fn register(registry: &mut ToolRegistry, user: &Arc<User>, session_id: &str) {
    registry.register(Arc::new(MemoryTool {
        user: user.clone(),
        writer: Some(MemoryWriter {
            session_id: session_id.to_owned(),
        }),
    }));
}

/// `memory` that only recalls and reads history, for agents that only read
/// memories.
pub(crate) fn recall_tool(user: &Arc<User>) -> Arc<dyn Tool> {
    Arc::new(MemoryTool {
        user: user.clone(),
        writer: None,
    })
}

impl MemoryTool {
    fn actions(&self) -> &'static [Action] {
        match self.writer {
            Some(_) => Action::ALL,
            None => Action::READS,
        }
    }

    /// The writer for an action that changes memories, refused without one.
    fn writer(&self, action: Action) -> Result<&MemoryWriter> {
        self.writer
            .as_ref()
            .ok_or_else(|| anyhow!("memory {} is not available here", action.as_str()))
    }
}

const SCOPE_HELP: &str = "What the fact is about: `user` (the default) for the user and their life, \
    `machine:<name>` for one of their machines by its name in the machines list, `project:<name>` for a \
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
                let Some(node) = machines::find(&self.0, name) else {
                    bail!("the user has no machine called {name:?}; the machines list shows them");
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
        "content":{"type":"string","description":"One short self-contained sentence expressing one independently useful fact."},
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
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        match self.writer {
            Some(_) => {
                "The user's memories. `recall` searches them by meaning, best first; an empty query lists \
                 the most recent. Each result has an id, a source (user, or learned from a chat, email or \
                 slack), its last change date, days for temporary facts, a quote when known, and a scope \
                 when it is about a machine, project or account. `store` remembers one short \
                 self-contained sentence for one independently useful fact, naming its subject; distinct \
                 facts belong in separate memories. Identical text in the same scope is reused; \
                 give valid_from/valid_until for facts that hold for a while, like \
                 a trip. Never store secrets. `update` replaces a memory's text by id with the full new \
                 sentence (days and scope stay unless given), `forget` deletes one by id, and `history` \
                 lists its earlier versions, newest first, for when something changed."
            }
            None => {
                "The user's memories, read only. `recall` searches them by meaning, best first; an empty \
                 query lists the most recent. Each result has an id, a source, its last change date, days \
                 for temporary facts and a scope when it is about a machine, project or account. \
                 `history` lists a memory's earlier versions by id, newest first."
            }
        }
    }
    fn parameters(&self) -> Value {
        let actions: Vec<&str> = self.actions().iter().map(|a| a.as_str()).collect();
        let mut properties = json!({
            "action": {"type": "string", "enum": actions},
            "query": {"type": "string", "description": "recall: what to look for."},
            "category": {"type": "string", "enum": MEMORY_CATEGORIES, "description": "recall: puts this category first."},
            "id": {"type": "string", "description": "The id recall gave."},
            "scope": {"type": "string", "description": format!("{SCOPE_HELP} For recall, only memories in that scope; leave out to search them all.")}
        });
        if self.writer.is_some()
            && let (Some(properties), Some(fact)) = (
                properties.as_object_mut(),
                fact_schema(&[], json!({}))["properties"].as_object(),
            )
        {
            properties.extend(fact.clone());
        }
        json!({"type": "object", "properties": properties, "required": ["action"], "additionalProperties": false})
    }
    fn humanize(&self, args: &Value) -> String {
        match action(args) {
            Ok(Action::Recall) => "Recalling memories",
            Ok(Action::Store) => "Remembering",
            Ok(Action::Update) => "Updating a memory",
            Ok(Action::Forget) => "Forgetting a memory",
            Ok(Action::History) => "Checking a memory's history",
            Err(_) => "Checking memories",
        }
        .to_owned()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let action = match action(&ctx.args) {
            Ok(action) if self.actions().contains(&action) => action,
            Ok(action) => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("memory {} is not available here.", action.as_str()),
                ));
            }
            Err(error) => return Ok(ToolResult::failure(ctx.tool_call_id, error.to_string())),
        };
        match action {
            Action::Recall => self.recall(&ctx).await,
            Action::Store => self.store(&ctx, self.writer(action)?).await,
            Action::Update => self.update(&ctx, self.writer(action)?).await,
            Action::Forget => {
                self.writer(action)?;
                let forgotten = self.user.forget(arg(&ctx.args, "id")?).await?;
                reply(&ctx, json!({"forgotten": forgotten}))
            }
            Action::History => self.history(&ctx).await,
        }
    }
}

/// The `action` of a call.
fn action(args: &Value) -> Result<Action> {
    let name = arg(args, "action")?;
    name.parse().map_err(|_| {
        anyhow!("unknown action {name:?}; use recall, store, update, forget or history")
    })
}

impl MemoryTool {
    async fn store(&self, ctx: &ToolContext<'_>, writer: &MemoryWriter) -> Result<ToolResult> {
        let names = ScopeNames::load(&self.user).await?;
        let mut fact = fact(&ctx.args, MemorySource::User {}, Some(writer.origin()))?;
        fact.scope = names.arg(&ctx.args)?;
        let content = fact.content;
        let remembered = self.user.remember(fact).await?;
        let change = if remembered.replaced {
            "updated"
        } else {
            "stored"
        };
        // The content lets the chat show what was remembered.
        reply(
            ctx,
            json!({change: true, "id": remembered.id, "content": content}),
        )
    }

    async fn recall(&self, ctx: &ToolContext<'_>) -> Result<ToolResult> {
        let query = opt_arg(&ctx.args, "query").unwrap_or_default();
        // An empty query is a request to see what Beep knows.
        let limit = if query.trim().is_empty() { 100 } else { 30 };
        let category = opt_arg(&ctx.args, "category");
        let names = ScopeNames::load(&self.user).await?;
        let scope = names.arg(&ctx.args)?;
        let scope = scope.as_ref();
        let mut hits = self
            .user
            .search_memories(query, category, scope, limit)
            .await?;
        // A model can guess a category the memory was not filed under, such
        // as "place" for a hotel stay filed under "plan", so the rest follow.
        if category.is_some() && hits.len() < limit {
            for hit in self.user.search_memories(query, None, scope, limit).await? {
                if hits.len() == limit {
                    break;
                }
                if !hits.iter().any(|h| h.id == hit.id) {
                    hits.push(hit);
                }
            }
        }
        reply(
            ctx,
            hits.iter()
                .map(|h| h.to_json(names.name(&h.scope).as_deref()))
                .collect(),
        )
    }

    async fn update(&self, ctx: &ToolContext<'_>, writer: &MemoryWriter) -> Result<ToolResult> {
        let names = ScopeNames::load(&self.user).await?;
        let mut fact = fact(&ctx.args, MemorySource::User {}, Some(writer.origin()))?;
        fact.scope = names.arg(&ctx.args)?;
        let content = fact.content;
        let updated = self.user.update_memory(arg(&ctx.args, "id")?, fact).await?;
        reply(ctx, json!({"updated": updated, "content": content}))
    }

    async fn history(&self, ctx: &ToolContext<'_>) -> Result<ToolResult> {
        let history = self.user.memory_history(arg(&ctx.args, "id")?).await?;
        reply(
            ctx,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;
    use flashmind::core::CancellationToken;

    async fn call(tool: &dyn Tool, args: Value) -> ToolResult {
        let cancel = CancellationToken::new();
        tool.execute(ToolContext::new("call", args, None, &cancel))
            .await
            .expect("the store answers")
    }

    #[tokio::test]
    async fn stores_then_recalls_and_a_reader_cannot_write() {
        let db = TestDb::new().await.expect("test database");
        let user = User::for_test(db.store().await.expect("store"), "alice");
        let mut registry = ToolRegistry::new();
        register(&mut registry, &user, "chat");
        let memory = registry.get(NAME).expect("memory registered").clone();

        let stored = call(
            memory.as_ref(),
            json!({"action": "store", "content": "Alice prefers tea."}),
        )
        .await;
        assert!(stored.is_success(), "{}", stored.output());
        let recalled = call(memory.as_ref(), json!({"action": "recall", "query": ""})).await;
        assert!(recalled.output().contains("Alice prefers tea."));

        let reader = recall_tool(&user);
        assert_eq!(
            reader.parameters()["properties"]["action"]["enum"],
            json!(["recall", "history"])
        );
        let refused = call(
            reader.as_ref(),
            json!({"action": "store", "content": "Alice prefers coffee."}),
        )
        .await;
        assert!(!refused.is_success());
        let recalled = call(reader.as_ref(), json!({"action": "recall", "query": ""})).await;
        assert!(!recalled.output().contains("coffee"));
    }
}
