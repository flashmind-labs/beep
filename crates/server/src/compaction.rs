//! Summarizes the older turns of long sessions once they go quiet, so later
//! turns send less history. Only the model's context is compacted; the
//! transcript the phone shows keeps every message.

use crate::{
    app::App,
    store::{SessionRef, Store},
};
use anyhow::Result;
use flashmind::types::{LlmProvider, Model};
use std::{sync::Arc, time::Duration};

/// A session is compacted only when its last prompt reached `compact_at`
/// tokens and nobody has used it for this long.
pub const IDLE_BEFORE_COMPACT: Duration = Duration::from_secs(15 * 60);
/// Limit for models with a context window of at least `LARGE_WINDOW`.
const LARGE_WINDOW: u32 = 1_000_000;
const LARGE_LIMIT: u32 = 300_000;
const LIMIT: u32 = 200_000;
/// Assumed window when the provider does not report one, as flashmind does.
const DEFAULT_WINDOW: u32 = 128_000;
/// Recent history kept verbatim after the summary.
const KEEP_RECENT_TOKENS: u32 = 8_000;
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

/// The model and provider that write the summary.
pub struct Summarizer<'a> {
    pub provider: &'a dyn LlmProvider,
    pub model: &'a Model,
}

/// Token limit for a model's context window: 300k from a 1M window up,
/// otherwise 200k, or 80% of a smaller window so the summary runs before the
/// window fills mid-turn.
pub fn compact_at(window: Option<u32>) -> u32 {
    let window = window.unwrap_or(DEFAULT_WINDOW);
    if window >= LARGE_WINDOW {
        LARGE_LIMIT
    } else {
        LIMIT.min(window / 5 * 4)
    }
}

/// Compacts every session that is both idle and over the token limit.
pub async fn compact_idle(store: &Store, summarizer: Summarizer<'_>) {
    let window = summarizer.provider.context_window(summarizer.model).await;
    let sessions = match store
        .sessions_to_compact(compact_at(window), IDLE_BEFORE_COMPACT)
        .await
    {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("unable to list sessions to compact: {error}");
            return;
        }
    };
    for session in sessions {
        match compact(store, &summarizer, &session).await {
            Ok(true) => log::info!("session compacted; session={}", session.id),
            Ok(false) => {}
            Err(error) => log::warn!("compaction failed: {error}; session={}", session.id),
        }
    }
}

/// Returns whether the session's context was replaced.
async fn compact(store: &Store, summarizer: &Summarizer<'_>, session: &SessionRef) -> Result<bool> {
    let Some(before) = store.context(&session.user, &session.id).await? else {
        return Ok(false);
    };
    let mut context = before.conversation.clone();
    let summary = context
        .compact_with_llm_keeping_tokens(summarizer.provider, summarizer.model, KEEP_RECENT_TOKENS)
        .await?;
    if summary.is_none() {
        // Nothing old enough to summarize. Saving still resets the token count,
        // so the session is not picked again until a new turn measures it.
        store
            .save_compacted(session, &before, &before.conversation)
            .await?;
        return Ok(false);
    }
    // A run that started meanwhile changed the context; the next idle sweep retries.
    store.save_compacted(session, &before, &context).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{CreateRun, FinishedTurn};
    use async_trait::async_trait;
    use flashmind::{
        core::{Conversation, ConversationEntry},
        types::{CompletionRequest, CompletionStream, FinishReason, Provider, StreamEvent},
    };

    struct SummaryStub;

    #[async_trait]
    impl LlmProvider for SummaryStub {
        fn name(&self) -> &str {
            "test"
        }
        fn provider(&self) -> Provider {
            Provider::OpenAi
        }
        fn complete(&self, _request: CompletionRequest) -> CompletionStream {
            Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta("## Goal\nPlan a trip.".into()));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
            })
        }
    }

    fn long_chat() -> Conversation {
        let mut chat = Conversation::with_system("system");
        for turn in 0..4 {
            chat.add(ConversationEntry::user(format!(
                "turn {turn} {}",
                "x".repeat(9_000)
            )));
            chat.add(ConversationEntry::assistant(format!("reply {turn}")));
        }
        chat
    }

    async fn save(store: &Store, session: &str, tokens: u32) -> SessionRef {
        let CreateRun::Created(run) = store
            .create_run("alice", session, &format!("req-{session}"), "hi")
            .await
            .unwrap()
        else {
            panic!("run was not created");
        };
        let chat = long_chat();
        let turn = FinishedTurn {
            transcript: &chat,
            context: &chat,
            prompt_tokens: tokens,
        };
        store
            .finish("alice", &run, Some(turn), "completed")
            .await
            .unwrap();
        SessionRef {
            user: "alice".into(),
            id: session.into(),
        }
    }

    async fn make_idle(store: &Store, session: &str) {
        let session = session.to_owned();
        store
            .raw()
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "UPDATE sessions SET updated_at='2000-01-01T00:00:00Z' WHERE id=?",
                    [session],
                )
            })
            .await
            .unwrap();
    }

    #[test]
    fn limit_follows_the_context_window() {
        assert_eq!(compact_at(Some(2_000_000)), 300_000);
        assert_eq!(compact_at(Some(1_000_000)), 300_000);
        assert_eq!(compact_at(Some(400_000)), 200_000);
        assert_eq!(compact_at(Some(200_000)), 160_000);
        assert_eq!(compact_at(None), 102_400);
    }

    #[tokio::test]
    async fn compacts_only_long_idle_sessions_and_keeps_the_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        let model: Model = "openai:test".parse().unwrap();
        let summarizer = || Summarizer {
            provider: &SummaryStub,
            model: &model,
        };

        let long_idle = save(&store, "long-idle", 150_000).await;
        let long_active = save(&store, "long-active", 150_000).await;
        let short_idle = save(&store, "short-idle", 50_000).await;
        make_idle(&store, &long_idle.id).await;
        make_idle(&store, &short_idle.id).await;

        compact_idle(&store, summarizer()).await;

        let context = |s: SessionRef| {
            let store = store.clone();
            async move {
                store
                    .context(&s.user, &s.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .conversation
            }
        };
        let compacted = context(long_idle.clone()).await;
        assert!(compacted.entries().iter().any(|e| e.is_summary()));
        assert!(compacted.entries().len() < long_chat().entries().len());
        // The two newest turns are kept verbatim after the summary.
        assert_eq!(compacted.entries().last().unwrap().content(), "reply 3");

        let transcript = store
            .conversation(&long_idle.user, &long_idle.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transcript.entries().len(), long_chat().entries().len());

        for untouched in [long_active, short_idle] {
            let context = context(untouched).await;
            assert!(!context.entries().iter().any(|e| e.is_summary()));
        }

        // Compacting resets the token count, so the next sweep leaves it alone.
        let picked = store
            .sessions_to_compact(compact_at(None), IDLE_BEFORE_COMPACT)
            .await
            .unwrap();
        assert!(picked.is_empty());
    }

    #[tokio::test]
    async fn a_run_during_compaction_wins() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        let session = save(&store, "busy", 40_000).await;
        let before = store
            .context(&session.user, &session.id)
            .await
            .unwrap()
            .unwrap();

        // A new turn lands after the sweep read the context.
        let mut newer = long_chat();
        newer.add(ConversationEntry::user("one more"));
        let CreateRun::Created(run) = store
            .create_run("alice", "busy", "req-busy-2", "one more")
            .await
            .unwrap()
        else {
            panic!("run was not created");
        };
        let turn = FinishedTurn {
            transcript: &newer,
            context: &newer,
            prompt_tokens: 40_000,
        };
        store
            .finish("alice", &run, Some(turn), "completed")
            .await
            .unwrap();

        let replaced = store
            .save_compacted(&session, &before, &Conversation::new())
            .await
            .unwrap();
        assert!(!replaced);
        let context = store.context("alice", "busy").await.unwrap().unwrap();
        assert_eq!(
            context.conversation.entries().last().unwrap().content(),
            "one more"
        );
    }
}
