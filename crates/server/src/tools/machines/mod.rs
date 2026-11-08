//! Tools that use the user's machines. The server reaches none itself: a desktop of the
//! account runs the work over its mesh (`relay`).

use crate::{account::user::User, app::App};
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub mod relay;

/// Registers the machine tools, which a desktop runs.
pub(crate) fn register(registry: &mut ToolRegistry, app: &Arc<App>, user: &Arc<User>) {
    registry.register(Arc::new(ExecTool {
        desktops: app.desktops.clone(),
        user: user.id().to_owned(),
    }));
}

/// A machine's name as one word, such as `build-box`.
pub(crate) fn command_name(name: &str) -> String {
    name.trim().to_lowercase().replace(' ', "-")
}

/// Runs a shell command on one of the user's machines, through a desktop.
pub(crate) struct ExecTool {
    desktops: relay::Desktops,
    /// The user's id.
    user: String,
}

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Run a shell command on one of the user's machines. One of the user's desktops runs it \
         over their mesh after the user allows it, and returns its output."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command." },
                "in": { "type": "string", "description": "The machine to run it on, by name. The desktop picks one when left out." },
                "cwd": { "type": "string", "description": "The folder to start in; the home folder when left out." }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let text = |name: &str| {
            ctx.args
                .get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let Some(command) = text("command") else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "command is required.".to_owned(),
            ));
        };
        let work = relay::Work::Exec {
            command,
            machine: text("in"),
            cwd: text("cwd"),
        };
        Ok(match relay::run(&self.desktops, &self.user, work).await {
            Ok(output) => ToolResult::success(ctx.tool_call_id, output),
            Err(error) => ToolResult::failure(ctx.tool_call_id, error),
        })
    }
}

#[cfg(test)]
#[path = "../../tests/tools_machines.rs"]
mod tests;
