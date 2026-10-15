//! What the server adds to a turn after the model: the memory score and the
//! calls it shows like the model's own.

use super::App;
use crate::{
    decision::Criteria,
    live::LiveRun,
    memories::{self, Exchange, Hit, Turn},
    protocol::RunEvent,
    tools,
};
use chrono::{DateTime, Utc};
use flashmind::{
    core::{Conversation, ConversationEntry, EntryKind},
    types::ToolCall,
};
use std::{collections::HashSet, fmt::Write};
use uuid::Uuid;

/// Above this Jev score the learner reads the turn; below it nothing is stored.
pub(super) const MEMORY_THRESHOLD: f64 = 0.6;
const MEMORY_QUESTION: &str = "Does this exchange reveal something about the user worth remembering for future chats that is not already remembered?";
const MEMORY_CRITERIA: Criteria = Criteria {
    yes: "Anything lasting about the user, even mentioned in passing inside a request: plans and goals \
(buying a home, changing jobs), preferences and dislikes, diet, beliefs, religion, politics, health, money \
and debts, people in their life, places, work, hobbies, routines, or such a fact the assistant found in their \
data at their request; or a correction or a request to forget",
    no: "Small talk, thanks, pure questions about the world, conversions, translations, one-off tasks \
that say nothing about the user, or facts already remembered",
};

/// Above this Jev score the user corrected the approach, and the chat is
/// reviewed for skills straight away.
pub(super) const CORRECTION_THRESHOLD: f64 = 0.7;
const CORRECTION_QUESTION: &str = "In the latest turn, does the user correct how the assistant went about a task in the earlier turn?";
const CORRECTION_CRITERIA: Criteria = Criteria {
    yes: "The user says the approach was wrong or asks for the task done another way: other tools, apps, \
sites, shops or people, another order of steps, another format, length or tone, or a step it should have \
taken or skipped",
    no: "A new request, a follow-up question, thanks, more details for the same task, or a correction of a \
fact about the user or the world that does not change how the task is done",
};

impl App {
    /// Asks Jev whether the turns in `state`, from [`correction_state`],
    /// show the user correcting how Beep went about a task. False without
    /// Jev.
    pub(super) async fn corrected(&self, state: Option<String>) -> bool {
        let (Some(decision), Some(state)) = (self.decision(), state) else {
            return false;
        };
        match decision
            .probability_with(&state, CORRECTION_QUESTION, &CORRECTION_CRITERIA)
            .await
        {
            Ok(probability) => probability >= CORRECTION_THRESHOLD,
            Err(error) => {
                log::warn!("correction score failed: {error}");
                false
            }
        }
    }

    /// Asks Jev how likely the exchange holds something worth remembering.
    /// Shown under the reply while we compare it with what the learner saves.
    pub(super) async fn memory_score(&self, state: String) -> Option<f64> {
        let decision = self.decision()?;
        match decision
            .probability_with(&state, MEMORY_QUESTION, &MEMORY_CRITERIA)
            .await
        {
            Ok(probability) => Some(probability),
            Err(error) => {
                log::warn!("memory score failed: {error}");
                None
            }
        }
    }
}

/// What Jev reads to tell whether `latest` corrects `earlier`, or `None`
/// without an earlier turn to correct.
pub(super) fn correction_state(earlier: Option<&Turn>, latest: &Turn) -> Option<String> {
    Some(format!(
        "Earlier turn:\n{}\n\nLatest turn:\n{latest}",
        earlier?
    ))
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

/// `conversation` without the follow-up cards the server saved as
/// `suggest_replies` calls. The model has no such tool, and seeing earlier
/// turns call it, it calls it too.
pub(super) fn drop_follow_ups(conversation: &mut Conversation) {
    let mut dropped = HashSet::new();
    let entries = conversation.entries_mut();
    for entry in entries.iter_mut() {
        if let EntryKind::Assistant {
            tool_calls: Some(calls),
            ..
        } = &mut entry.kind
        {
            calls.retain(|call| {
                let follow_up = call.name == tools::SUGGEST_REPLIES;
                if follow_up {
                    dropped.insert(call.id.clone());
                }
                !follow_up
            });
        }
    }
    if dropped.is_empty() {
        return;
    }
    entries.retain(|entry| match &entry.kind {
        EntryKind::Tool { call_id, .. } => !dropped.contains(call_id),
        EntryKind::Assistant {
            content,
            tool_calls,
            reasoning,
        } => {
            !content.is_empty()
                || reasoning.as_ref().is_some_and(|text| !text.is_empty())
                || tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
        }
        _ => true,
    });
    for entry in entries.iter_mut() {
        if let EntryKind::Assistant { tool_calls, .. } = &mut entry.kind
            && tool_calls.as_ref().is_some_and(Vec::is_empty)
        {
            *tool_calls = None;
        }
    }
}

/// `conversation` without the text the agent wrote from `since` on. Its
/// calls stay, so a card it showed in that time is kept.
pub(super) fn drop_text_since(conversation: &mut Conversation, since: DateTime<Utc>) {
    let entries = conversation.entries_mut();
    entries.retain_mut(|entry| {
        if entry.timestamp < since {
            return true;
        }
        let EntryKind::Assistant {
            content,
            tool_calls,
            ..
        } = &mut entry.kind
        else {
            return true;
        };
        content.clear();
        tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
    });
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
                actions: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: serde_json::json!({}),
        }
    }

    #[test]
    fn follow_ups_leave_the_context() {
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user("weather?"));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![call("w", "weather")],
            None,
        ));
        conversation.add(ConversationEntry::tool("w", "sunny"));
        conversation.add(ConversationEntry::assistant("Sunny."));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![call("s", tools::SUGGEST_REPLIES), call("m", "remember")],
            None,
        ));
        conversation.add(ConversationEntry::tool("s", "{}"));
        conversation.add(ConversationEntry::tool("m", "saved"));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![call("t", tools::SUGGEST_REPLIES)],
            None,
        ));
        conversation.add(ConversationEntry::tool("t", "{}"));

        drop_follow_ups(&mut conversation);

        let names: Vec<String> = conversation
            .entries()
            .iter()
            .map(|entry| match &entry.kind {
                EntryKind::Assistant {
                    tool_calls: Some(calls),
                    ..
                } => calls
                    .iter()
                    .map(|call| call.name.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                EntryKind::Tool { call_id, .. } => format!("result {call_id}"),
                _ => entry.content().to_owned(),
            })
            .collect();
        assert_eq!(
            names,
            [
                "weather?", "weather", "result w", "Sunny.", "remember", "result m"
            ]
        );
    }

    #[test]
    fn a_quiet_pass_keeps_its_calls_but_not_its_text() {
        let since = Utc::now();
        let mut earlier = ConversationEntry::assistant("Go to Isetan.");
        earlier.timestamp = since - TimeDelta::seconds(1);
        let mut conversation = Conversation::new();
        conversation.add(earlier);
        conversation.add(ConversationEntry::system_message("Show the map."));
        conversation.add(ConversationEntry::assistant_with_tool_calls(
            "Opening the map.",
            vec![call("m", "open_maps")],
            None,
        ));
        conversation.add(ConversationEntry::tool("m", "shown"));
        conversation.add(ConversationEntry::assistant("The map is below."));

        drop_text_since(&mut conversation, since);

        let kept: Vec<(String, usize)> = conversation
            .entries()
            .iter()
            .map(|entry| match &entry.kind {
                EntryKind::Assistant {
                    content,
                    tool_calls,
                    ..
                } => (content.clone(), tool_calls.as_ref().map_or(0, Vec::len)),
                _ => (entry.content().to_owned(), 0),
            })
            .collect();
        assert_eq!(
            kept,
            [
                ("Go to Isetan.".to_owned(), 0),
                ("Show the map.".to_owned(), 0),
                (String::new(), 1),
                ("shown".to_owned(), 0),
            ]
        );
    }
}
