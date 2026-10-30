//! How a woken or scheduled run says its reply is worth sending.

use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) const NAME: &str = "notify_user";

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
         never sees it."
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
            "Your reply will be sent. If you already wrote it, stop: writing it again \
             sends it twice. Otherwise write it now.",
        ))
    }
}
