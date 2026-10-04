use crate::{
    app::App,
    protocol::{DeviceAnswer, DeviceQuery},
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct ReadScreenTool {
    pub app: Arc<App>,
    pub run_id: String,
}

#[async_trait]
impl Tool for ReadScreenTool {
    fn name(&self) -> &str {
        "read_screen"
    }
    fn description(&self) -> &str {
        "Read the text on the user's phone screen, from the app open behind Beep's bubble. \
         Use it when the user refers to what they are looking at. It needs Beep's \
         accessibility service, which the user turns on in Beep's settings."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{},"additionalProperties":false})
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(15)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        match self
            .app
            .device_query(&self.run_id, DeviceQuery::ReadScreen {}, 10)
            .await
        {
            Ok(DeviceAnswer::Ok { output }) => Ok(ToolResult::success(ctx.tool_call_id, output)),
            Ok(DeviceAnswer::Err { message }) => Ok(ToolResult::failure(ctx.tool_call_id, message)),
            Err(e) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Could not reach the user's device: {e}"),
            )),
        }
    }
}
