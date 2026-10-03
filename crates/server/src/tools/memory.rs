use crate::{
    memories::Memories,
    store::{MemorySource, NewMemory, Store},
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct MemoryTool {
    pub store: Store,
    pub memories: Memories,
    pub user: String,
    pub name: &'static str,
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        match self.name {
            "memory_store" => {
                "Remember a durable fact about the user under a short snake_case key, as one self-contained sentence. Storing under an existing key replaces it; a fact that repeats an existing memory updates that one. Never store secrets."
            }
            "memory_recall" => {
                "Search the user's memories by meaning, best matches first, with the date each was last changed. An empty query lists the most recent. Optionally filter by category."
            }
            "memory_forget" => "Forget a user memory by its exact key.",
            "memory_update" => "Update an existing memory's content or category by its exact key.",
            _ => "Get the current UTC date and time.",
        }
    }
    fn parameters(&self) -> Value {
        match self.name {
            "memory_store" => {
                json!({"type":"object","properties":{
                    "key":{"type":"string"},
                    "content":{"type":"string"},
                    "category":{"type":"string","enum":["personal","preference","work","location","routine","general"],"description":"Memory category. Defaults to 'general'."}
                },"required":["key","content"],"additionalProperties":false})
            }
            "memory_recall" => {
                json!({"type":"object","properties":{
                    "query":{"type":"string"},
                    "category":{"type":"string","enum":["personal","preference","work","location","routine","general"],"description":"Filter by category."}
                },"required":["query"],"additionalProperties":false})
            }
            "memory_forget" => {
                json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"],"additionalProperties":false})
            }
            "memory_update" => {
                json!({"type":"object","properties":{
                    "key":{"type":"string","description":"The memory key to update."},
                    "content":{"type":"string","description":"The new content."},
                    "category":{"type":"string","enum":["personal","preference","work","location","routine","general"],"description":"Change the category."}
                },"required":["key","content"],"additionalProperties":false})
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
        let output = match self.name {
            "memory_store" => {
                let key = self
                    .memories
                    .remember(
                        &self.user,
                        NewMemory {
                            key: arg("key")?,
                            content: arg("content")?,
                            category: opt_arg("category"),
                            source: MemorySource::User,
                        },
                    )
                    .await?;
                json!({"stored":true,"key":key})
            }
            "memory_recall" => {
                let hits = self
                    .memories
                    .search(&self.user, arg("query")?, opt_arg("category"), 30)
                    .await?;
                hits.into_iter()
                    .map(|h| json!({"key":h.key,"content":h.content,"category":h.category,"updated":h.date}))
                    .collect()
            }
            "memory_forget" => {
                json!({"forgotten":self.store.forget(&self.user,arg("key")?).await?})
            }
            "memory_update" => {
                let updated = self
                    .store
                    .update_memory(
                        &self.user,
                        arg("key")?,
                        arg("content")?,
                        opt_arg("category"),
                    )
                    .await?;
                json!({"updated": updated})
            }
            _ => json!({"utc":chrono::Utc::now().to_rfc3339()}),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
