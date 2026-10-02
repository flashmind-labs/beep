use crate::{
    app::App,
    protocol::{DeviceAnswer, DeviceQuery},
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct ContactsTool {
    pub app: Arc<App>,
    pub run_id: String,
}

#[async_trait]
impl Tool for ContactsTool {
    fn name(&self) -> &str {
        "query_contacts"
    }
    fn description(&self) -> &str {
        "Search the user's phone contacts by name. Returns matching contacts with phone numbers and email addresses."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name to search for (partial match). Leave empty to list recent contacts."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(15)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let name = ctx.args.get("name").and_then(Value::as_str).unwrap_or("");
        let query = DeviceQuery::Contacts {
            name: name.to_owned(),
        };
        match self.app.device_query(&self.run_id, query, 10).await {
            Ok(DeviceAnswer::Ok { output }) => Ok(ToolResult::success(ctx.tool_call_id, output)),
            Ok(DeviceAnswer::Err { message }) => Ok(ToolResult::failure(ctx.tool_call_id, message)),
            Err(e) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Could not reach the user's device: {e}"),
            )),
        }
    }
}
