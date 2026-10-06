//! What the server adds to a turn after the model: the memory score and the
//! calls it shows like the model's own.

use super::App;
use crate::{
    live::LiveRun,
    memories::{self, Exchange, Hit, Turn},
    protocol::RunEvent,
};
use chrono::{DateTime, Utc};
use flashmind::{
    core::{Conversation, ConversationEntry},
    types::ToolCall,
};
use std::fmt::Write;
use uuid::Uuid;

/// Above this Jev score the learner reads the turn; below it nothing is stored.
pub(super) const MEMORY_THRESHOLD: f64 = 0.8;
const MEMORY_QUESTION: &str = "Does this exchange reveal a lasting fact about the user worth remembering for future chats that is not already remembered, correct one that is, or ask to forget one? Such facts are their name, preferences, people, places, work, routines or plans, whether the user said it or the assistant found it in their data at their request. Answer no for small talk, one-off questions, facts only about the world, or facts the already remembered list covers.";

impl App {
    /// Asks Jev how likely the exchange holds something worth remembering.
    /// Shown under the reply while we compare it with what the learner saves.
    pub(super) async fn memory_score(&self, state: String) -> Option<f64> {
        let decision = self.tools.decision.as_ref()?;
        match decision.probability(&state, MEMORY_QUESTION).await {
            Ok(probability) => Some(probability),
            Err(error) => {
                log::warn!("memory score failed: {error}");
                None
            }
        }
    }
}

/// A call the server made for the turn after the model finished, shown and
/// saved like one the model made.
pub(super) struct ServerCall {
    pub(super) name: String,
    pub(super) arguments: serde_json::Value,
    pub(super) description: String,
    pub(super) output: String,
}

pub(super) fn add_server_calls(
    conversation: &mut Conversation,
    live: &LiveRun,
    calls: Vec<ServerCall>,
) {
    if calls.is_empty() {
        return;
    }
    let calls: Vec<(ToolCall, ServerCall)> = calls
        .into_iter()
        .map(|call| {
            let tool_call = ToolCall {
                id: Uuid::new_v4().to_string(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            };
            (tool_call, call)
        })
        .collect();
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        calls
            .iter()
            .map(|(tool_call, _)| tool_call.clone())
            .collect(),
        None,
    ));
    for (tool_call, call) in calls {
        live.push(RunEvent::ToolStarted {
            name: call.name.clone(),
            call_id: tool_call.id.clone(),
            description: call.description,
        });
        live.push(RunEvent::ToolResult {
            name: call.name,
            call_id: tool_call.id.clone(),
            output: call.output.clone(),
            success: true,
        });
        conversation.add(ConversationEntry::tool(tool_call.id, call.output));
    }
}

/// What Jev reads to score a turn: what is already remembered, then the
/// exchange, so a fact that is already known does not score high.
pub(super) fn memory_state(known: &[Hit], exchange: &Exchange) -> String {
    let mut state = String::from("Already remembered:\n");
    if known.is_empty() {
        state.push_str("(nothing related)\n");
    }
    for hit in known {
        let _ = writeln!(state, "- {}", hit.content);
    }
    let _ = write!(state, "\n{}", exchange.latest);
    state
}

/// The last few turns before `turn_started`, oldest first, for the memory
/// extractor to resolve references in the latest one.
pub(super) fn earlier_turns(conversation: &Conversation, turn_started: DateTime<Utc>) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for entry in conversation.entries() {
        if entry.timestamp >= turn_started {
            break;
        }
        if entry.is_user() {
            turns.push(Turn {
                message: entry.content().to_owned(),
                reply: String::new(),
            });
        } else if entry.is_assistant()
            && !entry.content().is_empty()
            && let Some(turn) = turns.last_mut()
        {
            if !turn.reply.is_empty() {
                turn.reply.push('\n');
            }
            turn.reply.push_str(entry.content());
        }
    }
    let skip = turns.len().saturating_sub(memories::LEARN_CONTEXT_TURNS);
    turns.split_off(skip)
}
