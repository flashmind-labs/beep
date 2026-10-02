use crate::{
    app::App,
    protocol::{BrowserCommand, DeviceAnswer, DeviceQuery, ScrollDirection},
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct OpenBrowserTool;

#[async_trait]
impl Tool for OpenBrowserTool {
    fn name(&self) -> &str {
        "open_browser"
    }
    fn description(&self) -> &str {
        "Open a URL in the in-app WebView on the user's Android device. Optionally extract page content."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to open"
                },
                "extract_content": {
                    "type": "boolean",
                    "description": "Whether to read the page content back"
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let url = ctx.args.get("url").and_then(Value::as_str).unwrap_or("");
        let extract_content = ctx
            .args
            .get("extract_content")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let output = json!({
            "action": "open_browser",
            "url": url,
            "extract_content": extract_content,
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

pub(crate) struct BrowserTool {
    pub app: Arc<App>,
    pub run_id: String,
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }
    fn description(&self) -> &str {
        "Control the in-app browser on the user's phone. Navigate to URLs, click elements, type text, scroll, and read page content using CSS selectors."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["navigate", "click", "type", "scroll", "get_text", "get_page_info"],
                    "description": "The browser action to perform"
                },
                "url": {
                    "type": "string",
                    "description": "URL to navigate to (required for navigate)"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the target element (for click, type, get_text)"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type into the selected element (for type action)"
                },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down"],
                    "description": "Scroll direction (for scroll action, default: down)"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(20)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |name: &str| {
            ctx.args
                .get(name)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let command = match ctx.args.get("action").and_then(Value::as_str) {
            Some("navigate") => match arg("url") {
                Some(url) => BrowserCommand::Navigate { url },
                None => return Ok(failure(ctx, "url is required for navigate action")),
            },
            Some("click") => match arg("selector") {
                Some(selector) => BrowserCommand::Click { selector },
                None => return Ok(failure(ctx, "selector is required for click action")),
            },
            Some("type") => match (arg("selector"), arg("text")) {
                (Some(selector), Some(text)) => BrowserCommand::Type { selector, text },
                _ => {
                    return Ok(failure(
                        ctx,
                        "selector and text are required for type action",
                    ));
                }
            },
            Some("get_text") => match arg("selector") {
                Some(selector) => BrowserCommand::GetText { selector },
                None => return Ok(failure(ctx, "selector is required for get_text action")),
            },
            Some("scroll") => BrowserCommand::Scroll {
                direction: match arg("direction").as_deref() {
                    Some("up") => ScrollDirection::Up {},
                    _ => ScrollDirection::Down {},
                },
            },
            Some("get_page_info") => BrowserCommand::GetPageInfo {},
            _ => return Ok(failure(ctx, "unknown browser action")),
        };

        let query = DeviceQuery::Browser { command };
        match self.app.device_query(&self.run_id, query, 15).await {
            Ok(DeviceAnswer::Ok { output }) => Ok(ToolResult::success(ctx.tool_call_id, output)),
            Ok(DeviceAnswer::Err { message }) => Ok(ToolResult::failure(ctx.tool_call_id, message)),
            Err(e) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Could not reach the user's device: {e}"),
            )),
        }
    }
}

fn failure(ctx: ToolContext<'_>, message: &str) -> ToolResult {
    ToolResult::failure(ctx.tool_call_id, message)
}
