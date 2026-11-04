//! `machine_log`: a machine's recent beep-node log, for debugging the machine.

use super::{Machines, Need, in_parameter};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};

/// The lines a call gets unless it asks for others.
const DEFAULT_LINES: u32 = 200;

const DESCRIPTION: &str = "Read the recent log of beep-node on one of the user's machines, to \
    debug the machine itself: its link to Beep, sync, agent sessions. Gives the last `lines` \
    lines (default 200, at most 1000), oldest first.";

pub(crate) struct MachineLogTool {
    pub(crate) machines: Machines,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    lines: Option<u32>,
    #[serde(default, rename = "in")]
    machine: Option<String>,
}

#[async_trait]
impl Tool for MachineLogTool {
    fn name(&self) -> &str {
        "machine_log"
    }
    fn description(&self) -> &str {
        DESCRIPTION
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lines": { "type": "integer", "minimum": 1, "maximum": 1000 },
                "in": in_parameter("Omit for the user's own Linux environment.")
            },
            "additionalProperties": false
        })
    }
    fn humanize(&self, _: &Value) -> String {
        "Reading a machine's log".to_owned()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        let picked = match self
            .machines
            .pick(args.machine.as_deref(), Need::Shell)
            .await
        {
            Ok(picked) => picked,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e)),
        };
        let lines = args.lines.unwrap_or(DEFAULT_LINES);
        Ok(match picked.online.log(lines).await {
            Ok(text) if text.is_empty() => {
                ToolResult::success(ctx.tool_call_id, "Nothing logged lately.".to_owned())
            }
            Ok(text) => ToolResult::success(ctx.tool_call_id, text),
            Err(e) => ToolResult::failure(ctx.tool_call_id, format!("{e:#}")),
        })
    }
}
