use crate::{
    memories::Memories,
    protocol::MemorySource,
    store::{Fact, Store, Validity},
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::NaiveDate;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::sync::Arc;

const CATEGORIES: [&str; 6] = [
    "personal",
    "preference",
    "work",
    "location",
    "routine",
    "general",
];

/// What every memory tool needs to reach one user's memories.
#[derive(Clone)]
struct Access {
    store: Store,
    memories: Memories,
    user: String,
}

struct MemoryStoreTool(Access);
struct MemoryRecallTool(Access);
struct MemoryForgetTool(Access);
struct MemoryUpdateTool(Access);

pub fn register(registry: &mut ToolRegistry, store: Store, memories: Memories, user: &str) {
    let access = Access {
        store,
        memories,
        user: user.to_owned(),
    };
    registry.register(Arc::new(MemoryStoreTool(access.clone())));
    registry.register(Arc::new(MemoryRecallTool(access.clone())));
    registry.register(Arc::new(MemoryForgetTool(access.clone())));
    registry.register(Arc::new(MemoryUpdateTool(access)));
}

fn arg<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    opt_arg(args, name).ok_or_else(|| anyhow!("missing string argument: {name}"))
}

fn opt_arg<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
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

/// The fact the user asked to store or update, from the shared argument names.
fn fact(args: &Value) -> Result<Fact<'_>> {
    let valid = Validity {
        from: date(args, "valid_from")?,
        until: date(args, "valid_until")?,
    };
    Ok(Fact {
        content: arg(args, "content")?,
        category: opt_arg(args, "category"),
        source: MemorySource::User {},
        valid: (valid != Validity::default()).then_some(valid),
        embedding: None,
    })
}

fn fact_schema(required: &[&str], extra: Value) -> Value {
    let mut properties = json!({
        "content":{"type":"string","description":"One short self-contained sentence."},
        "category":{"type":"string","enum":CATEGORIES},
        "valid_from":{"type":"string","description":"YYYY-MM-DD the fact starts holding, for temporary facts like a trip."},
        "valid_until":{"type":"string","description":"YYYY-MM-DD of the last day the fact holds. Leave out for lasting facts."}
    });
    if let (Some(properties), Some(extra)) = (properties.as_object_mut(), extra.as_object()) {
        properties.extend(extra.clone());
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn reply(ctx: &ToolContext<'_>, output: Value) -> Result<ToolResult> {
    Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
}

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &str {
        "memory_store"
    }
    fn description(&self) -> &str {
        "Remember a fact about the user as one short self-contained sentence that names its subject. A fact that says the same as an existing memory replaces it. Give valid_from/valid_until for facts that only hold for a while, like a trip or an appointment. Never store secrets."
    }
    fn parameters(&self) -> Value {
        fact_schema(&["content"], json!({}))
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let Access { memories, user, .. } = &self.0;
        let remembered = memories.remember(user, fact(&ctx.args)?).await?;
        let change = if remembered.replaced {
            "updated"
        } else {
            "stored"
        };
        reply(&ctx, json!({change: true, "id": remembered.id}))
    }
}

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &str {
        "memory_recall"
    }
    fn description(&self) -> &str {
        "Search the user's memories by meaning, best first. An empty query lists the most recent. Each result has an id, a source (user, learned from a chat, or email), its last change date and, for temporary facts, its days."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string"},
            "category":{"type":"string","enum":CATEGORIES,"description":"Filter by category."}
        },"required":["query"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let Access { memories, user, .. } = &self.0;
        let query = arg(&ctx.args, "query")?;
        // An empty query is a request to see what Beep knows.
        let limit = if query.trim().is_empty() { 100 } else { 30 };
        let hits = memories
            .search(user, query, opt_arg(&ctx.args, "category"), limit)
            .await?;
        reply(&ctx, hits.iter().map(|h| h.to_json()).collect())
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
        let Access { store, user, .. } = &self.0;
        let forgotten = store.forget(user, arg(&ctx.args, "id")?).await?;
        reply(&ctx, json!({"forgotten": forgotten}))
    }
}

#[async_trait]
impl Tool for MemoryUpdateTool {
    fn name(&self) -> &str {
        "memory_update"
    }
    fn description(&self) -> &str {
        "Replace a memory's text by the id memory_recall gave, with the full new sentence. Days and category stay unless given."
    }
    fn parameters(&self) -> Value {
        fact_schema(&["id", "content"], json!({"id":{"type":"string"}}))
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let Access { memories, user, .. } = &self.0;
        let updated = memories
            .update(user, arg(&ctx.args, "id")?, fact(&ctx.args)?)
            .await?;
        reply(&ctx, json!({"updated": updated}))
    }
}
