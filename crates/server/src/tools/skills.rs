//! Loads a skill the context lists, for the user's chats or for a group's.

use super::memory::{arg, reply};
use crate::store::{SkillOwner, Store};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct SkillViewTool {
    pub store: Store,
    pub owner: SkillOwner,
}

#[async_trait]
impl Tool for SkillViewTool {
    fn name(&self) -> &str {
        "skill_view"
    }
    fn description(&self) -> &str {
        "Load one of the skills the context lists, by its name: the steps and preferences to follow for that task. Load it before starting the task it covers."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "name":{"type":"string","description":"The skill's name as listed."}
        },"required":["name"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let name = arg(&ctx.args, "name")?;
        match self.store.use_skill(&self.owner, name).await? {
            Some(skill) => reply(
                &ctx,
                json!({"name": skill.name, "description": skill.description, "body": skill.body}),
            ),
            None => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("No skill called \"{name}\"."),
            )),
        }
    }
}
