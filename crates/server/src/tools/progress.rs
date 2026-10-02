use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct TaskProgressTool;

#[async_trait]
impl Tool for TaskProgressTool {
    fn name(&self) -> &str {
        "task_progress"
    }
    fn description(&self) -> &str {
        "Report progress on a multi-step task. Call this to show the user a visual checklist of steps and their statuses as you work through them."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "description": "List of steps with their current status",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": { "type": "string", "description": "Short description of this step" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "failed"], "description": "Current status of this step" }
                        },
                        "required": ["label", "status"]
                    }
                }
            },
            "required": ["steps"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let steps = ctx.args.get("steps").cloned().unwrap_or_else(|| json!([]));
        let output = json!({
            "type": "task_progress",
            "steps": steps,
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
