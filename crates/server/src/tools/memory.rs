use crate::{
    protocol::MemorySource,
    store::{Fact, MEMORY_CATEGORIES, Origin, Validity},
    user::User,
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
fn fact(args: &Value, origin: Origin) -> Result<Fact<'_>> {
    let valid = Validity {
        from: date(args, "valid_from")?,
        until: date(args, "valid_until")?,
    };
    Ok(Fact {
        content: arg(args, "content")?,
        category: None,
        source: MemorySource::User {},
        valid: (valid != Validity::default()).then_some(valid),
        origin: Some(origin),
    })
}

fn fact_schema(required: &[&str], extra: Value) -> Value {
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
        let fact = fact(&ctx.args, self.0.origin())?;
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
        "Search the user's memories by meaning, best first. An empty query lists the most recent. Each result has an id, a source (user, learned from a chat, email or slack), its last change date, for temporary facts its days, and when known a quote of the words it came from."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string"},
            "category":{"type":"string","enum":MEMORY_CATEGORIES,"description":"Filter by category. When nothing matches, the search runs again without it."}
        },"required":["query"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let query = arg(&ctx.args, "query")?;
        // An empty query is a request to see what Beep knows.
        let limit = if query.trim().is_empty() { 100 } else { 30 };
        let category = opt_arg(&ctx.args, "category");
        let mut hits = self.0.search_memories(query, category, limit).await?;
        // A model can guess a category the memory was not filed under, such
        // as "place" for a hotel stay filed under "plan".
        if hits.is_empty() && category.is_some() {
            hits = self.0.search_memories(query, None, limit).await?;
        }
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
        "Replace a memory's text by the id memory_recall gave, with the full new sentence. Days stay unless given."
    }
    fn parameters(&self) -> Value {
        fact_schema(&["id", "content"], json!({"id":{"type":"string"}}))
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let fact = fact(&ctx.args, self.0.origin())?;
        let content = fact.content;
        let updated = self
            .0
            .user
            .update_memory(arg(&ctx.args, "id")?, fact)
            .await?;
        reply(&ctx, json!({"updated": updated, "content": content}))
    }
}
