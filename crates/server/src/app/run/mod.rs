//! Runs one turn of the agent and saves it.

mod turn;

use super::research;
use crate::store::RunStatus;
use crate::{
    apps::permissions::{self},
    projects::turn::{self as project_turn},
};
use flashmind::{
    core::{Conversation, ConversationEntry},
    tools::{
        firecrawl::{WebCrawlTool, WebExtractTool, WebMapTool, WebScrapeTool},
        search_cache::{SearchCacheRef, SearchResultCache},
        search_read::WebSearchReadTool,
    },
    types::{LlmError, LlmErrorKind, ToolRegistry, tool::Tool},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;

/// The system prompt of every chat turn.
const ASSISTANT_PROMPT: &str = include_str!("../../../prompts/assistant.md");

/// The longest any run may take; the repeat guards stop a looping model sooner.
const MAX_RUN: Duration = Duration::from_secs(15 * 60);
/// How many times a reply that stopped partway through the task is sent back to continue.
const MAX_CONTINUES: usize = 6;

/// How long a run may go without a single event from the model or a tool.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// The idle limit while a tool runs: crawls poll quietly for up to two minutes.
const TOOL_IDLE_TIMEOUT: Duration = Duration::from_secs(150);
/// What the user is told when the server gives up on a turn.
pub(super) const STALLED: &str = "The model stopped responding.";
pub(super) const TIMED_OUT: &str = "The run timed out.";
pub(super) const LOOPED: &str = "The model got stuck repeating itself and was stopped.";
/// A slow embedding must not hold up the conversation indefinitely.
const MEMORY_RECALL_TIMEOUT: Duration = Duration::from_secs(5);

/// How much of a scheduled task the reply checks read.
const SCHEDULED_TASK_CHARS: usize = 160;

/// What the user saw of a run they stopped, kept in `conversation` so the
/// chat shows it and the model reads it next turn: each call still running
/// answered as stopped, then the text and reasoning streamed since the last
/// tool calls, ending in a note that the user stopped it, or that a server
/// restart did (`restarted`). Returns the stopped calls.
pub(super) fn keep_stopped(
    conversation: &mut Conversation,
    said: String,
    thought: String,
    restarted: bool,
) -> Vec<String> {
    let open: Vec<String> = permissions::open_calls(conversation)
        .into_iter()
        .map(|call| call.id)
        .collect();
    let (call_note, note) = if restarted {
        (RESTARTED_CALL, RESTARTED)
    } else {
        (STOPPED_CALL, STOPPED)
    };
    permissions::close_open_calls(conversation, call_note);
    let said = said.trim_end();
    let content = if said.is_empty() {
        note.to_owned()
    } else {
        format!("{said}\n\n{note}")
    };
    let thought = Some(thought).filter(|thought| !thought.trim().is_empty());
    conversation.add(ConversationEntry::assistant_with_reasoning(
        content, thought,
    ));
    open
}

/// A call's result when the user stopped the run while it ran.
const STOPPED_CALL: &str = "Stopped by the user before it finished.";
/// The note a stopped turn ends with.
pub(super) const STOPPED: &str = "_Stopped_";
/// A call's result, and the note its turn ends with, when the server
/// restarted while it ran.
const RESTARTED_CALL: &str = "Cut off by a server restart before it finished; it may have \
                              partly run.";
const RESTARTED: &str = "_Cut off by a server restart_";

/// Firecrawl's page tools, with a per-run cache so `web_search_read` only reaches this run's pages.
pub(super) fn register_web_tools(registry: &mut ToolRegistry, key: &str) {
    for tool in web_tools(key) {
        registry.register(tool);
    }
}

/// Firecrawl's page tools, sharing one cache.
pub(super) fn web_tools(key: &str) -> Vec<Arc<dyn Tool>> {
    let cache: SearchCacheRef = Arc::new(RwLock::new(SearchResultCache::new()));
    vec![
        Arc::new(WebScrapeTool::new(key.to_owned())),
        Arc::new(WebMapTool::new(key.to_owned())),
        Arc::new(WebCrawlTool::new(key.to_owned(), cache.clone())),
        Arc::new(WebExtractTool::new(key.to_owned())),
        Arc::new(WebSearchReadTool::new(cache)),
    ]
}

/// The server-owned system prompt: a research lead's, a project agent's, or the assistant's.
fn system_prompt(research: bool, project: bool) -> &'static str {
    match (research, project) {
        (true, _) => research::LEAD_PROMPT,
        (false, true) => project_turn::PROJECT_PROMPT,
        (false, false) => ASSISTANT_PROMPT,
    }
}

/// What the user is told when the server gave up on a turn itself.
fn gave_up(status: RunStatus) -> Option<&'static str> {
    match status {
        RunStatus::Stalled => Some(STALLED),
        RunStatus::TimedOut => Some(TIMED_OUT),
        RunStatus::Looped => Some(LOOPED),
        _ => None,
    }
}

/// What the user is told about a failed model request, by its cause.
fn failure_message(error: &str) -> &'static str {
    match LlmError::classify(error).kind {
        LlmErrorKind::Configuration => {
            "The model request failed. Check server provider configuration."
        }
        LlmErrorKind::ContextLengthExceeded => {
            "This chat is too long for the model, even after summarizing it."
        }
        LlmErrorKind::InvalidRequest | LlmErrorKind::MalformedToolCall => {
            "The model rejected something in this chat, such as a file it cannot read."
        }
        LlmErrorKind::StreamError | LlmErrorKind::RateLimited => {
            "The model provider is busy or unreachable. Try again in a moment."
        }
        LlmErrorKind::ContentFiltered => "The model refused to answer this.",
        _ => "The model request failed.",
    }
}

fn languages_line(languages: &str) -> String {
    format!(
        "Languages: {languages} Reply in the language of the user's message. When it mixes \
         languages, or a stray word or a misspelling makes it look like one they do not \
         speak, reply in English if they speak it, else in the first language they speak."
    )
}

#[cfg(test)]
mod tests;
