use crate::user::User;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct GetLocationTool {
    pub user: Arc<User>,
}

#[async_trait]
impl Tool for GetLocationTool {
    fn name(&self) -> &str {
        "get_location"
    }
    fn description(&self) -> &str {
        "Get the user's last known location: coordinates, accuracy and when it was taken. Requires the user to have enabled location sharing on their device."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{},"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let output = match self.user.get_location().await? {
            Some(loc) => json!({
                "latitude": loc.latitude,
                "longitude": loc.longitude,
                "accuracy": loc.accuracy,
                "timezone": loc.timezone,
                "updated_at": loc.updated_at,
            }),
            None => json!({
                "message": "Location data will be available when the user enables it on their device"
            }),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
