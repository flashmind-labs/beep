//! How a woken run says its reply is worth sending (see `crate::wake`).

use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const NAME: &str = "notify_user";

pub(crate) struct NotifyUserTool {
    /// Set once the model calls the tool; the run reads it when it ends.
    pub notify: Arc<AtomicBool>,
}

#[async_trait]
impl Tool for NotifyUserTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Send your final reply to the user as a notification. Call it only when the event is \
         worth interrupting them for. Without this call the reply is dropped and the user \
         never sees this chat."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        self.notify.store(true, Ordering::SeqCst);
        Ok(ToolResult::success(
            ctx.tool_call_id,
            "Your reply will be sent. Write it now.",
        ))
    }
}
