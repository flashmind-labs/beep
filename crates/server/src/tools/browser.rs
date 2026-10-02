use crate::app::App;
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
        let action = ctx.args.get("action").and_then(Value::as_str).unwrap_or("");

        if action == "navigate" {
            let url = ctx.args.get("url").and_then(Value::as_str).unwrap_or("");
            if url.is_empty() {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "url is required for navigate action",
                ));
            }
        }

        if matches!(action, "click" | "type" | "get_text") {
            let selector = ctx
                .args
                .get("selector")
                .and_then(Value::as_str)
                .unwrap_or("");
            if selector.is_empty() {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "selector is required for click, type, and get_text actions",
                ));
            }
        }

        if action == "type" {
            let text = ctx.args.get("text").and_then(Value::as_str).unwrap_or("");
            if text.is_empty() {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "text is required for type action",
                ));
            }
        }

        let mut query = json!({ "action": "browser_command" });
        query["browser_action"] = json!(action);
        if let Some(v) = ctx.args.get("url") {
            query["url"] = v.clone();
        }
        if let Some(v) = ctx.args.get("selector") {
            query["selector"] = v.clone();
        }
        if let Some(v) = ctx.args.get("text") {
            query["text"] = v.clone();
        }
        if let Some(v) = ctx.args.get("direction") {
            query["direction"] = v.clone();
        }

        match self.app.device_query(&self.run_id, query, 15).await {
            Ok(result) => {
                if result.get("error").is_some() {
                    Ok(ToolResult::failure(ctx.tool_call_id, result.to_string()))
                } else {
                    Ok(ToolResult::success(ctx.tool_call_id, result.to_string()))
                }
            }
            Err(e) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Could not reach the user's device: {e}"),
            )),
        }
    }
}
