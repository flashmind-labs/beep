use crate::store::Store;
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::sync::Arc;

struct MemoryTool {
    store: Store,
    user: String,
    name: &'static str,
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        match self.name {
            "memory_store" => {
                "Remember or update a durable user fact by stable key. Never store secrets."
            }
            "memory_recall" => {
                "Find the user's memories by literal substring. Empty query lists up to 50 memories."
            }
            "memory_forget" => "Forget a user memory by its exact key.",
            _ => "Get the current UTC date and time.",
        }
    }
    fn parameters(&self) -> Value {
        match self.name {
            "memory_store" => {
                json!({"type":"object","properties":{"key":{"type":"string"},"content":{"type":"string"}},"required":["key","content"],"additionalProperties":false})
            }
            "memory_recall" => {
                json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false})
            }
            "memory_forget" => {
                json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"],"additionalProperties":false})
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
        let output = match self.name {
            "memory_store" => {
                self.store
                    .remember(&self.user, arg("key")?, arg("content")?)
                    .await?;
                json!({"stored":true})
            }
            "memory_recall" => self.store.recall(&self.user, arg("query")?).await?,
            "memory_forget" => {
                json!({"forgotten":self.store.forget(&self.user,arg("key")?).await?})
            }
            _ => json!({"utc":chrono::Utc::now().to_rfc3339()}),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

pub fn registry(store: Store, user: &str) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for name in ["memory_store", "memory_recall", "memory_forget", "get_time"] {
        registry.register(Arc::new(MemoryTool {
            store: store.clone(),
            user: user.into(),
            name,
        }));
    }
    registry
}
