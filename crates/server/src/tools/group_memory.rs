//! Memory tools for a group chat; each hit is recorded as a secret for the leak gate.

use super::memory::{fact, fact_schema, opt_arg, reply};
use crate::{
    leak::Secret,
    protocol::MemorySource,
    store::{MEMORY_CATEGORIES, Scope},
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

/// Whether a memory may enter a group run; mail and Slack stay in the member's own chats.
pub(crate) fn shareable(source: MemorySource) -> bool {
    matches!(source, MemorySource::User {} | MemorySource::Learned {})
}

/// `memory` in a group: `recall` across the members, `store` for one.
struct GroupMemoryTool {
    members: Arc<[Member]>,
    actor: Member,
    secrets: Secrets,
}

/// Registers recall and store across `members`; `actor` asked.
pub fn register(
    registry: &mut ToolRegistry,
    members: Arc<[Member]>,
    actor: Member,
    secrets: Secrets,
) {
    registry.register(Arc::new(GroupMemoryTool {
        members,
        actor,
        secrets,
    }));
}

fn names(members: &[Member]) -> Vec<&str> {
    members.iter().map(|member| member.name.as_str()).collect()
}

#[async_trait]
impl Tool for GroupMemoryTool {
    fn name(&self) -> &str {
        super::memory::NAME
    }
    fn description(&self) -> &str {
        "The members' memories. `recall` searches their private memories by meaning; each result says who it is about. Use them to tailor suggestions, never state or hint at them in the chat. An empty query lists recent ones. `store` remembers a fact about a member: something they said in the chat, a plan the group agreed that involves them, or what they asked you to remember. One short self-contained sentence that names them. Store a shared plan once per member it involves. `about` defaults to the member who asked. Never store secrets, or guesses about someone."
    }
    fn parameters(&self) -> Value {
        let mut schema = fact_schema(
            &["action"],
            json!({
                "action":{"type":"string","enum":["recall","store"]},
                "query":{"type":"string","description":"recall: what to look for."},
                "about":{"type":"string","enum":names(&self.members),"description":"recall: only this member's memories. store: whose memory this is, by default the member who asked."},
                "category":{"type":"string","enum":MEMORY_CATEGORIES,"description":"recall: only this category."}
            }),
        );
        if let Some(content) = schema.pointer_mut("/properties/content/description") {
            *content = json!("store: one short self-contained sentence that names the member.");
        }
        schema
    }
    fn humanize(&self, args: &Value) -> String {
        match opt_arg(args, "action") {
            Some("store") => "Remembering",
            _ => "Recalling memories",
        }
        .to_owned()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        match opt_arg(&ctx.args, "action") {
            Some("recall") => self.recall(&ctx).await,
            Some("store") => self.store(&ctx).await,
            _ => Ok(ToolResult::failure(
                ctx.tool_call_id,
                "Give `action`: recall or store.",
            )),
        }
    }
}

impl GroupMemoryTool {
    async fn recall(&self, ctx: &ToolContext<'_>) -> Result<ToolResult> {
        let query = opt_arg(&ctx.args, "query").unwrap_or_default();
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
                .search_memories(query, category, Some(&Scope::User), RECALL_LIMIT)
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
        reply(ctx, Value::Array(found))
    }

    async fn store(&self, ctx: &ToolContext<'_>) -> Result<ToolResult> {
        let member = match opt_arg(&ctx.args, "about") {
            Some(name) => match self.members.iter().find(|m| m.name == name) {
                Some(member) => member,
                None => {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        format!("No member is called {name}."),
                    ));
                }
            },
            None => &self.actor,
        };
        member
            .user
            .remember(fact(&ctx.args, MemorySource::Learned {}, None)?)
            .await?;
        reply(ctx, json!({"stored": true, "about": member.name}))
    }
}
