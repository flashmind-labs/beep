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
        "Show a URL to the user in the in-app browser on their phone. To read or act on the page yourself, use the browser tool."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to open"
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let url = ctx.args.get("url").and_then(Value::as_str).unwrap_or("");
        let output = json!({ "action": "open_browser", "url": url });
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
        "Use the in-app browser on the user's phone, which they watch while you work. \
         Work in a loop: navigate, then read_page to get the page text and a numbered list of \
         links, buttons and fields, then click or type by that number, then read_page again. \
         Numbers change whenever the page changes, so read_page before each click or type. \
         navigate, back, forward, reload, click and type with submit wait for the page to load."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["navigate", "read_page", "click", "type", "scroll", "back", "forward", "reload", "wait_for", "get_text", "get_page_info"],
                    "description": "The browser action to perform"
                },
                "url": {
                    "type": "string",
                    "description": "URL to navigate to (required for navigate)"
                },
                "index": {
                    "type": "integer",
                    "description": "Element number from the last read_page (for click and type)"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector, for wait_for and get_text, or for click and type when there is no index"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type (for type)"
                },
                "submit": {
                    "type": "boolean",
                    "description": "Submit the form after typing, like pressing Enter (for type)"
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
        Some(30)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |name: &str| {
            ctx.args
                .get(name)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let index = ctx
            .args
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|i| u32::try_from(i).ok());
        let command = match ctx.args.get("action").and_then(Value::as_str) {
            Some("navigate") => match arg("url") {
                Some(url) => BrowserCommand::Navigate { url },
                None => return Ok(failure(ctx, "url is required for navigate action")),
            },
            Some("read_page") => BrowserCommand::ReadPage {},
            Some("click") => match (index, arg("selector")) {
                (Some(index), _) => BrowserCommand::ClickElement { index },
                (None, Some(selector)) => BrowserCommand::Click { selector },
                (None, None) => {
                    return Ok(failure(ctx, "index or selector is required for click"));
                }
            },
            Some("type") => {
                let Some(text) = arg("text") else {
                    return Ok(failure(ctx, "text is required for type action"));
                };
                let submit = ctx
                    .args
                    .get("submit")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                match (index, arg("selector")) {
                    (Some(index), _) => BrowserCommand::TypeElement {
                        index,
                        text,
                        submit,
                    },
                    (None, Some(selector)) => BrowserCommand::Type { selector, text },
                    (None, None) => {
                        return Ok(failure(ctx, "index or selector is required for type"));
                    }
                }
            }
            Some("get_text") => match arg("selector") {
                Some(selector) => BrowserCommand::GetText { selector },
                None => return Ok(failure(ctx, "selector is required for get_text action")),
            },
            Some("wait_for") => match arg("selector") {
                Some(selector) => BrowserCommand::WaitFor { selector },
                None => return Ok(failure(ctx, "selector is required for wait_for action")),
            },
            Some("scroll") => BrowserCommand::Scroll {
                direction: match arg("direction").as_deref() {
                    Some("up") => ScrollDirection::Up {},
                    _ => ScrollDirection::Down {},
                },
            },
            Some("back") => BrowserCommand::Back {},
            Some("forward") => BrowserCommand::Forward {},
            Some("reload") => BrowserCommand::Reload {},
            Some("get_page_info") => BrowserCommand::GetPageInfo {},
            _ => return Ok(failure(ctx, "unknown browser action")),
        };

        let query = DeviceQuery::Browser { command };
        match self.app.device_query(&self.run_id, query, 25).await {
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
