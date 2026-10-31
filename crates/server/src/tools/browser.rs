use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct OpenBrowserTool;

#[async_trait]
impl Tool for OpenBrowserTool {
    fn name(&self) -> &str {
        "open_browser"
    }

    // Acts on the phone, which handles one request at a time.
    fn concurrent(&self) -> bool {
        false
    }
    fn description(&self) -> &str {
        "Show a URL to the user in the in-app browser on their phone. This only shows the page; you cannot read or act on it."
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
