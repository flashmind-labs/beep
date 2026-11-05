//! Running one call of the model against an MCP server: asking the user first, then calling it
//! and marking what comes back as someone else's text.

use super::{McpHub, McpTool};
use crate::{
    Event,
    tools::{ToolResult, cap, untrusted},
};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

/// Characters of a result kept.
const RESULT_CHARS: usize = 12_000;
/// Longest the user is waited for.
const ANSWER_LIMIT: Duration = Duration::from_secs(600);
/// Arguments shown on the approval card, and characters of each.
const SHOWN_ARGS: usize = 8;
const SHOWN_CHARS: usize = 300;

/// A call waiting for the user's yes.
#[derive(Debug, Clone, PartialEq)]
pub struct Approval {
    pub query_id: String,
    pub server: String,
    /// The tool's own name.
    pub tool: String,
    pub fields: Vec<ApprovalField>,
}

/// One argument of the call, as a line of the card.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalField {
    pub label: String,
    pub value: String,
}

fn fields(arguments: &Value) -> Vec<ApprovalField> {
    let Some(object) = arguments.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .take(SHOWN_ARGS)
        .map(|(label, value)| {
            let text = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            ApprovalField {
                label: label.clone(),
                value: text.chars().take(SHOWN_CHARS).collect(),
            }
        })
        .collect()
}

/// Runs `call`'s `arguments` on `tool`'s server. `approve` takes the id of a question and gives
/// the user's answer once it comes; it is called before the question is sent, so the answer has
/// somewhere to land. No answer, or a no, is a failed result the model reads.
pub(crate) async fn run<A, G>(
    hub: &Arc<McpHub>,
    tool: &McpTool,
    arguments: Value,
    events: &Sender<Event>,
    approve: &A,
) -> ToolResult
where
    A: Fn(String) -> G,
    G: Future<Output = bool>,
{
    let query_id = Uuid::new_v4().to_string();
    let answer = approve(query_id.clone());
    let asked = Event::Approval(Approval {
        query_id,
        server: tool.server.clone(),
        tool: tool.tool.clone(),
        fields: fields(&arguments),
    });
    events.send(asked).await.ok();
    match tokio::time::timeout(ANSWER_LIMIT, answer).await {
        Ok(true) => {}
        Ok(false) => return ToolResult::failed("The user did not allow this call."),
        Err(_) => return ToolResult::failed("The user did not answer, so it was not run."),
    }
    match hub.call(tool, arguments).await {
        Ok(called) => ToolResult {
            text: untrusted(&tool.server, &cap(&called.text, RESULT_CHARS)),
            ok: called.ok,
        },
        Err(e) => ToolResult::failed(format!("{} did not answer: {e}.", tool.server)),
    }
}
