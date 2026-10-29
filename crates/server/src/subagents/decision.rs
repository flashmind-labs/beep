//! The tools a sub-agent ends its run with. Each records one typed decision;
//! the first one recorded is final.

use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use std::sync::{Arc, Mutex, PoisonError};

/// One way a run can end, as a tool the model calls.
pub struct Decision<D> {
    pub name: &'static str,
    pub description: &'static str,
    /// The tool's JSON schema.
    pub parameters: fn() -> Value,
    /// Reads the call's arguments. An error goes back to the model, which
    /// may call again.
    pub parse: fn(&Value) -> Result<D, String>,
}

impl<D> Clone for Decision<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D> Copy for Decision<D> {}

/// Where a run's decision lands.
pub(super) type Slot<D> = Arc<Mutex<Option<D>>>;

pub(super) fn take<D>(slot: &Slot<D>) -> Option<D> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

pub(super) struct DecisionTool<D> {
    pub(super) decision: Decision<D>,
    pub(super) slot: Slot<D>,
}

#[async_trait]
impl<D: Send + 'static> Tool for DecisionTool<D> {
    fn name(&self) -> &str {
        self.decision.name
    }
    fn description(&self) -> &str {
        self.decision.description
    }
    fn parameters(&self) -> Value {
        (self.decision.parameters)()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call = ctx.tool_call_id;
        let decided = match (self.decision.parse)(&ctx.args) {
            Ok(decided) => decided,
            Err(error) => return Ok(ToolResult::failure(call, format!("Bad arguments: {error}"))),
        };
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Ok(ToolResult::failure(call, "You already decided."));
        }
        *slot = Some(decided);
        Ok(ToolResult::success(call, "Recorded. You are done."))
    }
}
