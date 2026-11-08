//! Shrinks the model's context of long sessions: old tool output first, then summaries. The transcript keeps everything.

use crate::{
    agent::metering,
    agent::models::ModelChoices,
    app::App,
    apps::permissions,
    store::{SessionRef, Store},
};
use anyhow::Result;
use flashmind::{
    core::{Conversation, EntryKind},
    types::{LlmProvider, Model},
};
use std::{sync::Arc, time::Duration};

/// How long a session over `compact_at` must be idle before the sweep compacts it.
pub const IDLE_BEFORE_COMPACT: Duration = Duration::from_secs(15 * 60);
/// Prompt size that triggers compaction.
const LIMIT: u32 = 250_000;
/// Assumed window when the provider does not report one, as flashmind does.
const DEFAULT_WINDOW: u32 = 128_000;
/// Recent history kept verbatim, tool output included.
const KEEP_RECENT_TOKENS: u32 = 20_000;
/// Older tool output longer than this is replaced by a short stub.
const TRIM_OVER_CHARS: usize = 2000;
/// Rough size of a token, to tell whether trimming was enough.
const CHARS_PER_TOKEN: usize = 3;
const SWEEP_EVERY: Duration = Duration::from_secs(60);

/// Checks for sessions to compact every minute.
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_EVERY);
        loop {
            interval.tick().await;
            app.compact_idle_sessions().await;
        }
    });
}

/// Writes the summaries on each chat's last model; `default` covers sessions without one.
pub struct Summarizer<'a> {
    pub provider: &'a dyn LlmProvider,
    pub models: &'a ModelChoices,
    pub default: &'a Model,
}

impl Summarizer<'_> {
    /// The offered model `session`'s latest turn ran on, else the default.
    async fn model_for(&self, store: &Store, session: &SessionRef) -> &Model {
        match store.last_turn(session.id).await {
            Ok(Some(turn)) => self.models.find(&turn.model).unwrap_or(self.default),
            Ok(None) => self.default,
            Err(error) => {
                log::warn!(
                    "reading the last turn failed: {error:#}; session={}",
                    session.id
                );
                self.default
            }
        }
    }

    /// The lowest `compact_at` of any model that can be summarized.
    async fn lowest_limit(&self) -> u32 {
        let mut lowest = compact_at(self.provider.context_window(self.default).await);
        for model in self.models.all() {
            lowest = lowest.min(compact_at(self.provider.context_window(model).await));
        }
        lowest
    }
}

/// Token limit for a context window: 250k, or 80% of a smaller window.
pub fn compact_at(window: Option<u32>) -> u32 {
    LIMIT.min(window.unwrap_or(DEFAULT_WINDOW) / 5 * 4)
}

/// Compacts every idle session over its model's `compact_at`.
pub async fn compact_idle(store: &Store, summarizer: Summarizer<'_>) {
    let lowest = summarizer.lowest_limit().await;
    let sessions = match store.sessions_to_compact(lowest, IDLE_BEFORE_COMPACT).await {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("unable to list sessions to compact: {error}");
            return;
        }
    };
    for quiet in sessions {
        let model = summarizer.model_for(store, &quiet.session).await;
        let limit = compact_at(summarizer.provider.context_window(model).await);
        if quiet.context_tokens < limit {
            continue;
        }
        compact_session(store, summarizer.provider, model, &quiet.session, limit).await;
    }
}

/// Compacts one session and logs how it went.
pub async fn compact_session(
    store: &Store,
    provider: &dyn LlmProvider,
    model: &Model,
    session: &SessionRef,
    limit: u32,
) {
    match compact(store, provider, model, session, limit).await {
        Ok(true) => log::info!("session compacted; session={}", session.id),
        Ok(false) => {}
        Err(error) => log::warn!("compaction failed: {error}; session={}", session.id),
    }
}

/// Returns whether the session's context was replaced.
async fn compact(
    store: &Store,
    provider: &dyn LlmProvider,
    model: &Model,
    session: &SessionRef,
    limit: u32,
) -> Result<bool> {
    let Some(before) = store.session_context(session).await? else {
        return Ok(false);
    };
    // Waiting on the user: the open call must stay as it is until answered.
    if !permissions::open_calls(&before.conversation).is_empty() {
        return Ok(false);
    }
    let mut context = before.conversation.clone();
    // Recalled facts are not part of a summary; they can be found again.
    crate::app::memory_context::discard_saved(&mut context);
    let dropped = trim_tool_output(&mut context, KEEP_RECENT_TOKENS);
    // Half the limit, so the next turns do not trigger it again right away.
    if dropped > 0 && estimated_tokens(&context) < limit / 2 {
        log::debug!(
            "compaction: dropped {dropped} chars of tool output; session={}",
            session.id
        );
        return store.save_compacted(session, &before, &context).await;
    }
    let summary = metering::billed_to(
        &session.user,
        context.compact_with_llm_keeping_tokens(provider, model, KEEP_RECENT_TOKENS),
    )
    .await?;
    // With nothing old enough to summarize, saving still resets the token
    // count, so the session is not picked again until a new turn measures
    // it, and keeps whatever tool output was trimmed above. A run that
    // started meanwhile changed the context; the next idle sweep retries.
    let saved = store.save_compacted(session, &before, &context).await?;
    Ok(saved && summary.is_some())
}

/// Stubs long tool output older than the last `keep_recent_tokens`; returns the characters dropped.
fn trim_tool_output(conversation: &mut Conversation, keep_recent_tokens: u32) -> usize {
    let keep_chars = keep_recent_tokens as usize * CHARS_PER_TOKEN;
    let mut seen = 0;
    let mut dropped = 0;
    for entry in conversation.entries_mut().iter_mut().rev() {
        let recent = seen < keep_chars;
        seen += entry.content().len();
        if recent {
            continue;
        }
        if let EntryKind::Tool { output, .. } = &mut entry.kind
            && output.len() > TRIM_OVER_CHARS
        {
            dropped += output.len();
            *output = format!(
                "[{} KB of tool output dropped to save room. Call the tool again if it is needed.]",
                output.len().div_ceil(1024)
            );
        }
    }
    dropped
}

fn estimated_tokens(conversation: &Conversation) -> u32 {
    let chars: usize = conversation
        .entries()
        .iter()
        .map(|entry| entry.content().len())
        .sum();
    u32::try_from(chars / CHARS_PER_TOKEN).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "../tests/agent_compaction.rs"]
mod tests;
