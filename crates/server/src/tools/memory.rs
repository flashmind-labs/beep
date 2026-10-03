use crate::{
    memories::Memories,
    protocol::MemorySource,
    store::{Fact, Store, Validity},
};
use async_trait::async_trait;
use chrono::NaiveDate;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct MemoryTool {
    pub store: Store,
    pub memories: Memories,
    pub user: String,
    pub name: &'static str,
}

const CATEGORIES: [&str; 6] = [
    "personal",
    "preference",
    "work",
    "location",
    "routine",
    "general",
];

fn days_schema() -> (Value, Value) {
    (
        json!({"type":"string","description":"YYYY-MM-DD the fact starts holding, for temporary facts like a trip."}),
        json!({"type":"string","description":"YYYY-MM-DD of the last day the fact holds. Leave out for lasting facts."}),
    )
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        match self.name {
            "memory_store" => {
                "Remember a fact about the user as one short self-contained sentence that names its subject. A fact that says the same as an existing memory replaces it. Give valid_from/valid_until for facts that only hold for a while, like a trip or an appointment. Never store secrets."
            }
            "memory_recall" => {
                "Search the user's memories by meaning, best matches first. Each has an id, its source (user: the user said it; learned: picked up from a chat), the date it last changed and, for temporary facts, the days it holds. Facts whose days are over are left out. An empty query lists the most recent. Optionally filter by category."
            }
            "memory_forget" => "Forget a memory by the id memory_recall gave.",
            "memory_update" => {
                "Replace a memory's text by the id memory_recall gave, with the full new sentence. Days and category stay unless given."
            }
            _ => "Get the current UTC date and time.",
        }
    }
    fn parameters(&self) -> Value {
        let (from, until) = days_schema();
        match self.name {
            "memory_store" => {
                json!({"type":"object","properties":{
                    "content":{"type":"string"},
                    "category":{"type":"string","enum":CATEGORIES,"description":"Defaults to 'general'."},
                    "valid_from":from,
                    "valid_until":until
                },"required":["content"],"additionalProperties":false})
            }
            "memory_recall" => {
                json!({"type":"object","properties":{
                    "query":{"type":"string"},
                    "category":{"type":"string","enum":CATEGORIES,"description":"Filter by category."}
                },"required":["query"],"additionalProperties":false})
            }
            "memory_forget" => {
                json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false})
            }
            "memory_update" => {
                json!({"type":"object","properties":{
                    "id":{"type":"string"},
                    "content":{"type":"string","description":"The full new sentence."},
                    "category":{"type":"string","enum":CATEGORIES},
                    "valid_from":from,
                    "valid_until":until
                },"required":["id","content"],"additionalProperties":false})
            }
            _ => json!({"type":"object","properties":{},"additionalProperties":false}),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |name| {
            ctx.args
                .get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing string argument: {name}"))
        };
        let opt_arg = |name: &str| ctx.args.get(name).and_then(Value::as_str);
        let date = |name: &str| -> anyhow::Result<Option<NaiveDate>> {
            opt_arg(name)
                .filter(|d| !d.trim().is_empty())
                .map(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d"))
                .transpose()
                .map_err(|_| anyhow::anyhow!("{name} must be YYYY-MM-DD"))
        };
        let valid = || -> anyhow::Result<Option<Validity>> {
            let valid = Validity {
                from: date("valid_from")?,
                until: date("valid_until")?,
            };
            Ok((valid != Validity::default()).then_some(valid))
        };
        let output = match self.name {
            "memory_store" => {
                let fact = Fact {
                    content: arg("content")?,
                    category: opt_arg("category"),
                    source: MemorySource::User {},
                    valid: valid()?,
                    embedding: None,
                };
                let remembered = self.memories.remember(&self.user, fact).await?;
                if remembered.replaced {
                    json!({"updated":true,"id":remembered.id})
                } else {
                    json!({"stored":true,"id":remembered.id})
                }
            }
            "memory_recall" => {
                let query = arg("query")?;
                // An empty query is a request to see what Beep knows.
                let limit = if query.trim().is_empty() { 100 } else { 30 };
                let hits = self
                    .memories
                    .search(&self.user, query, opt_arg("category"), limit)
                    .await?;
                hits.iter().map(|h| h.to_json()).collect()
            }
            "memory_forget" => {
                json!({"forgotten":self.store.forget(&self.user,arg("id")?).await?})
            }
            "memory_update" => {
                let fact = Fact {
                    content: arg("content")?,
                    category: opt_arg("category"),
                    source: MemorySource::User {},
                    valid: valid()?,
                    embedding: None,
                };
                let updated = self.memories.update(&self.user, arg("id")?, fact).await?;
                json!({"updated": updated})
            }
            _ => json!({"utc":chrono::Utc::now().to_rfc3339()}),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
