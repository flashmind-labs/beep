//! `add_machine`: a one-time setup token for `beep-node setup <token>`.

use crate::nodes::{self, TokenRefusal};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

use super::Machines;

pub(crate) const NAME: &str = "add_machine";

/// How to put `beep-node` on a machine that lacks it.
const INSTALL: &str = "curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh";

pub(crate) struct AddMachineTool {
    pub(crate) machines: Machines,
}

#[async_trait]
impl Tool for AddMachineTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Add one of the user's computers as a machine Beep can use, such as a build server or a laptop. Makes a one-time setup token that works for 10 minutes and returns the command to run on that computer; give the user that command exactly. Use it when the user asks to add, link or connect a machine. `name` is what to call it; left out, the computer's host name is used."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "What to call the machine, such as \"build box\"" }
            },
            "additionalProperties": false
        })
    }
    fn humanize(&self, _args: &Value) -> String {
        "Making a setup token…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let name = ctx.args.get("name").and_then(Value::as_str);
        let Machines { app, user } = &self.machines;
        Ok(match nodes::create_token(app, user, name).await {
            Ok(made) => ToolResult::success(ctx.tool_call_id, instructions(&made.token.command())),
            Err(TokenRefusal::Failed(e)) => return Err(e),
            Err(refusal) => ToolResult::failure(ctx.tool_call_id, refusal.to_string()),
        })
    }
}

fn instructions(command: &str) -> String {
    format!(
        "Give the user this command to run on the machine within 10 minutes; it works once:\n\
         {command}\n\
         If beep-node is not installed there yet, they install it first with:\n\
         {INSTALL}\n\
         The machine links itself and starts; it then shows in the machines list."
    )
}
