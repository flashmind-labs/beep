use crate::{
    app::App,
    images,
    protocol::{DeviceAnswer, DeviceQuery},
    user::User,
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// Seconds the phone has to answer: the user confirms Android's capture prompt first.
const CAPTURE_SECS: u64 = 60;

pub(crate) struct ReadScreenTool {
    pub app: Arc<App>,
    pub run_id: String,
    pub user: Arc<User>,
}

/// The phone's answer: the screenshot as a JPEG data URI.
#[derive(Deserialize)]
struct Capture {
    image: String,
}

#[async_trait]
impl Tool for ReadScreenTool {
    fn name(&self) -> &str {
        "read_screen"
    }
    fn description(&self) -> &str {
        "Read what is on the user's phone screen, from the app open behind Beep's bubble. \
         Use it when the user refers to what they are looking at. The phone takes a \
         screenshot after the user allows it, and returns its text and a short description."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{},"additionalProperties":false})
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(CAPTURE_SECS + 60)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let id = ctx.tool_call_id;
        let output = match self
            .app
            .device_query(&self.run_id, DeviceQuery::ReadScreen {}, CAPTURE_SECS)
            .await
        {
            Ok(DeviceAnswer::Ok { output }) => output,
            Ok(DeviceAnswer::Err { message }) => return Ok(ToolResult::failure(id, message)),
            Err(e) => {
                return Ok(ToolResult::failure(
                    id,
                    format!("Could not reach the user's device: {e}"),
                ));
            }
        };
        let Some(image) = serde_json::from_str::<Capture>(&output)
            .ok()
            .and_then(|capture| images::parse_data_uri(&capture.image))
        else {
            return Ok(ToolResult::failure(id, "The phone sent no screenshot."));
        };
        let Some(reader) = self.app.screen_reader(&self.user).await else {
            return Ok(ToolResult::failure(
                id,
                "No model here can see images, so the screen cannot be read.",
            ));
        };
        Ok(match reader.read_screen(&image).await {
            Ok(text) => ToolResult::success(id, text),
            Err(e) => ToolResult::failure(id, format!("Could not read the screenshot: {e}")),
        })
    }
}
