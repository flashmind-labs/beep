//! Runs one turn of the agent and saves it.

mod turn;

use super::research;
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
    types::{ToolRegistry, tool::Tool},
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
/// A slow embedding must not hold up the conversation indefinitely.
const MEMORY_RECALL_TIMEOUT: Duration = Duration::from_secs(5);

/// How much of a scheduled task the reply checks read.
const SCHEDULED_TASK_CHARS: usize = 160;

/// What the user saw of a run they stopped, kept in `conversation` so the
/// chat shows it and the model reads it next turn: each call still running
/// answered as stopped, then the text and reasoning streamed since the last
/// tool calls, ending in a note that the user stopped it. Returns the
/// stopped calls.
pub(super) fn keep_stopped(
    conversation: &mut Conversation,
    said: String,
    thought: String,
) -> Vec<String> {
    let open: Vec<String> = permissions::open_calls(conversation)
        .into_iter()
        .map(|call| call.id)
        .collect();
    permissions::close_open_calls(conversation, STOPPED_CALL);
    let said = said.trim_end();
    let content = if said.is_empty() {
        STOPPED.to_owned()
    } else {
        format!("{said}\n\n{STOPPED}")
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

fn languages_line(languages: &str) -> String {
    format!(
        "Languages: {languages} Reply in the language of the user's message. When it mixes \
         languages, or a stray word or a misspelling makes it look like one they do not \
         speak, reply in English if they speak it, else in the first language they speak."
    )
}

#[cfg(test)]
mod tests;
