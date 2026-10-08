//! Memory tools for a group chat. Beep reads every member's memories, but
//! each hit is recorded as a secret so the leak gate can check the reply
//! against it. Facts are stored only for the member who asked, since the
//! others did not.

use super::memory::{arg, date, fact_schema, opt_arg, reply};
use crate::{
    leak::Secret,
    protocol::MemorySource,
    store::{Fact, MEMORY_CATEGORIES, Validity},
    user::User,
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, PoisonError};

/// Hits per member for a query; an empty query lists this many recent ones.
const RECALL_LIMIT: usize = 10;

/// A person in the group, by the name the others see.
#[derive(Clone)]
pub struct Member {
    pub name: String,
    pub user: Arc<User>,
}

/// The memories a group run has read, for the leak gate.
pub type Secrets = Arc<Mutex<Vec<Secret>>>;

/// Whether a memory may enter a group run. Mail and Slack hold other
/// people's words, so they stay in the member's own chats.
pub(crate) fn shareable(source: MemorySource) -> bool {
    matches!(source, MemorySource::User {} | MemorySource::Learned {})
}

struct GroupRecallTool {
    members: Arc<[Member]>,
    secrets: Secrets,
}

struct GroupStoreTool {
    actor: Member,
}

/// Registers recall across `members` and store for `actor`, who asked.
pub fn register(
    registry: &mut ToolRegistry,
    members: Arc<[Member]>,
    actor: Member,
    secrets: Secrets,
) {
    registry.register(Arc::new(GroupRecallTool { members, secrets }));
    registry.register(Arc::new(GroupStoreTool { actor }));
}

fn names(members: &[Member]) -> Vec<&str> {
    members.iter().map(|member| member.name.as_str()).collect()
}

#[async_trait]
impl Tool for GroupRecallTool {
    fn name(&self) -> &str {
        "memory_recall"
    }
    fn description(&self) -> &str {
        "Search the members' private memories by meaning. Each result says who it is about. Use them to tailor suggestions, never state or hint at them in the chat. An empty query lists recent ones."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string"},
            "about":{"type":"string","enum":names(&self.members),"description":"Only this member's memories."},
            "category":{"type":"string","enum":MEMORY_CATEGORIES}
        },"required":["query"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let query = arg(&ctx.args, "query")?;
        let about = opt_arg(&ctx.args, "about");
        let category = opt_arg(&ctx.args, "category");
        let mut found = Vec::new();
        let mut read = Vec::new();
        for member in self.members.iter() {
            if about.is_some_and(|name| name != member.name) {
                continue;
            }
            let hits = member
                .user
                .search_memories(query, category, RECALL_LIMIT)
                .await?;
            for hit in hits.into_iter().filter(|hit| shareable(hit.source)) {
                found.push(json!({
                    "about": member.name,
                    "content": hit.content,
                    "category": hit.category,
                    "updated": hit.date,
                }));
                read.push(Secret {
                    owner: member.name.clone(),
                    content: hit.content,
                });
            }
        }
        self.secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(read);
        reply(&ctx, Value::Array(found))
    }
}

#[async_trait]
impl Tool for GroupStoreTool {
    fn name(&self) -> &str {
        "memory_store"
    }
    fn description(&self) -> &str {
        "Remember a fact about the member who asked you, only when they said it in the chat or asked you to. One short self-contained sentence that names them. Never store anything about other members, and never secrets."
    }
    fn parameters(&self) -> Value {
        fact_schema(&["content"], json!({}))
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let member = &self.actor;
        let content = arg(&ctx.args, "content")?;
        let valid = Validity {
            from: date(&ctx.args, "valid_from")?,
            until: date(&ctx.args, "valid_until")?,
        };
        member
            .user
            .remember(Fact {
                content,
                category: None,
                source: MemorySource::Learned {},
                valid: (valid != Validity::default()).then_some(valid),
                origin: None,
            })
            .await?;
        reply(&ctx, json!({"stored": true, "about": member.name}))
    }
}
