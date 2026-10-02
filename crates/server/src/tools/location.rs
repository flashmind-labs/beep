use crate::store::Store;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct GetLocationTool {
    pub store: Store,
    pub user: String,
}

#[async_trait]
impl Tool for GetLocationTool {
    fn name(&self) -> &str {
        "get_location"
    }
    fn description(&self) -> &str {
        "Get the user's last known location (latitude, longitude). Requires the user to have enabled location sharing on their device."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{},"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let output = match self.store.get_location(&self.user).await? {
            Some(loc) => loc,
            None => json!({
                "message": "Location data will be available when the user enables it on their device"
            }),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
