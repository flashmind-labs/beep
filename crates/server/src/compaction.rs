//! Shrinks the model's context of long sessions: old tool output first, then summaries. The transcript keeps everything.

use crate::{
    app::App,
    metering,
    models::ModelChoices,
    permissions,
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
mod tests {
    use super::*;
    use crate::store::{CreateRun, FinishedTurn, RunStatus, TestDb, test_chat, test_steps};
    use crate::user::User;
    use async_trait::async_trait;
    use flashmind::{
        core::{Conversation, ConversationEntry},
        types::{CompletionRequest, CompletionStream, FinishReason, Provider, StreamEvent},
    };
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct SummaryStub {
        used: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl LlmProvider for SummaryStub {
        fn name(&self) -> &str {
            "test"
        }
        fn provider(&self) -> Provider {
            Provider::OpenAi
        }
        fn complete(&self, request: CompletionRequest) -> CompletionStream {
            self.used
                .lock()
                .expect("test lock")
                .push(request.model.to_string());
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
                "x".repeat(25_000)
            )));
            chat.add(ConversationEntry::assistant(format!("reply {turn}")));
        }
        chat
    }

    async fn save(store: &Store, name: &str, tokens: u32) -> SessionRef {
        save_chat(store, name, &long_chat(), tokens).await
    }

    async fn save_chat(store: &Store, name: &str, chat: &Conversation, tokens: u32) -> SessionRef {
        let session = test_chat(name);
        let CreateRun::Created(run) = User::for_test(store.clone(), "alice")
            .create_run(session, &format!("req-{name}"), "hi")
            .await
            .unwrap()
        else {
            panic!("run was not created");
        };
        let steps = test_steps(chat);
        let turn = FinishedTurn {
            context: chat,
            prompt_tokens: Some(tokens),
            steps: &steps,
            stands_alone: false,
            carried: Default::default(),
        };
        User::for_test(store.clone(), "alice")
            .finish(&run, Some(turn), RunStatus::Completed)
            .await
            .unwrap();
        SessionRef {
            user: "alice".into(),
            id: session,
        }
    }

    async fn make_idle(store: &Store, session: Uuid) {
        sqlx::query("UPDATE chats SET updated_at='2000-01-01T00:00:00Z' WHERE id=$1")
            .bind(session)
            .execute(store.pool())
            .await
            .unwrap();
    }

    #[test]
    fn old_tool_output_is_dropped_and_recent_output_kept() {
        let mut chat = Conversation::with_system("system");
        chat.add(ConversationEntry::user("Find the bank emails"));
        chat.add(ConversationEntry::tool("old", "x".repeat(300_000)));
        chat.add(ConversationEntry::tool("small", "ok"));
        chat.add(ConversationEntry::assistant("Found three."));
        chat.add(ConversationEntry::user("And the newest?"));
        chat.add(ConversationEntry::tool("new", "y".repeat(70_000)));
        let dropped = trim_tool_output(&mut chat, 20_000);
        assert_eq!(dropped, 300_000);
        let outputs: Vec<&str> = chat.entries().iter().map(|entry| entry.content()).collect();
        assert!(outputs[2].starts_with("[293 KB of tool output dropped"));
        assert_eq!(outputs[3], "ok");
        assert_eq!(outputs[4], "Found three.");
        assert_eq!(outputs[6].len(), 70_000);
        assert!(estimated_tokens(&chat) < 25_000);
    }

    #[test]
    fn limit_follows_the_context_window() {
        assert_eq!(compact_at(Some(1_000_000)), 250_000);
        assert_eq!(compact_at(Some(200_000)), 160_000);
        assert_eq!(compact_at(Some(128_000)), 102_400);
        assert_eq!(compact_at(None), 102_400);
    }

    #[tokio::test]
    async fn compaction_removes_recalled_memories_from_context() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let mut chat = long_chat();
        crate::app::memory_context::restore(
            &mut chat,
            &[ConversationEntry::memory(
                "A recalled fact.",
                "memory-1",
                0.8,
            )],
            true,
        );
        let session = save_chat(&store, "recalled", &chat, 150_000).await;
        let model: Model = "openai:test".parse().unwrap();
        assert!(
            compact(&store, &SummaryStub::default(), &model, &session, 102_400)
                .await
                .unwrap()
        );
        let compacted = store
            .session_context(&session)
            .await
            .unwrap()
            .unwrap()
            .conversation;
        assert!(
            !compacted
                .entries()
                .iter()
                .any(|entry| entry.content().contains("A recalled fact."))
        );
        assert!(compacted.entries().iter().any(|entry| entry.is_summary()));
    }

    #[tokio::test]
    async fn compacts_only_long_idle_sessions_and_keeps_the_messages() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let model: Model = "openai:test".parse().unwrap();
        let models = ModelChoices::only(model.clone());
        let stub = SummaryStub::default();
        let summarizer = || Summarizer {
            provider: &stub,
            models: &models,
            default: &model,
        };

        let long_idle = save(&store, "long-idle", 150_000).await;
        let long_active = save(&store, "long-active", 150_000).await;
        let short_idle = save(&store, "short-idle", 50_000).await;
        make_idle(&store, long_idle.id).await;
        make_idle(&store, short_idle.id).await;

        compact_idle(&store, summarizer()).await;

        let context = |s: SessionRef| {
            let store = store.clone();
            async move {
                User::for_test(store.clone(), &s.user)
                    .context(s.id)
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

        // The apps still get every message.
        let messages = store.chat_messages(long_idle.id).await.unwrap();
        assert_eq!(messages.len(), test_steps(&long_chat()).len());

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
    async fn trimmed_output_is_saved_when_there_is_nothing_to_summarize() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let model: Model = "openai:test".parse().unwrap();
        let mut chat = Conversation::with_system("system");
        chat.add(ConversationEntry::user("Find the bank emails"));
        chat.add(ConversationEntry::tool("old", "x".repeat(300_000)));
        chat.add(ConversationEntry::tool("new", "y".repeat(70_000)));
        let session = save_chat(&store, "one-message", &chat, 150_000).await;

        // Still over half the limit once trimmed, and one user message
        // leaves nothing to summarize.
        assert!(
            !compact(&store, &SummaryStub::default(), &model, &session, 40_000)
                .await
                .unwrap()
        );

        let context = User::for_test(store.clone(), &session.user)
            .context(session.id)
            .await
            .unwrap()
            .unwrap();
        let outputs: Vec<&str> = context
            .conversation
            .entries()
            .iter()
            .map(|entry| entry.content())
            .collect();
        assert!(outputs[2].starts_with("[293 KB of tool output dropped"));
    }

    #[tokio::test]
    async fn a_run_during_compaction_wins() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let session = save(&store, "busy", 40_000).await;
        let before = User::for_test(store.clone(), &session.user)
            .context(session.id)
            .await
            .unwrap()
            .unwrap();

        // A new turn lands after the sweep read the context.
        let mut newer = long_chat();
        newer.add(ConversationEntry::user("one more"));
        let CreateRun::Created(run) = User::for_test(store.clone(), "alice")
            .create_run(session.id, "req-busy-2", "one more")
            .await
            .unwrap()
        else {
            panic!("run was not created");
        };
        let steps = test_steps(&newer);
        let turn = FinishedTurn {
            context: &newer,
            prompt_tokens: Some(40_000),
            steps: &steps[steps.len() - 1..],
            stands_alone: false,
            carried: Default::default(),
        };
        User::for_test(store.clone(), "alice")
            .finish(&run, Some(turn), RunStatus::Completed)
            .await
            .unwrap();

        let replaced = store
            .save_compacted(&session, &before, &Conversation::new())
            .await
            .unwrap();
        assert!(!replaced);
        let context = User::for_test(store.clone(), "alice")
            .context(session.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            context.conversation.entries().last().unwrap().content(),
            "one more"
        );
    }

    async fn set_turn_model(store: &Store, session: Uuid, model: &str) {
        sqlx::query(
            "UPDATE turns SET model = $2 WHERE message_id IN
             (SELECT id FROM messages WHERE chat_id = $1)",
        )
        .bind(session)
        .bind(model)
        .execute(store.pool())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn idle_sessions_are_summarized_on_the_model_their_chat_ran_on() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let default: Model = "openai:default".parse().unwrap();
        let other: Model = "openai:other".parse().unwrap();
        let models = ModelChoices::only(default.clone()).offering(other.clone());

        let on_other = save(&store, "on-other", 150_000).await;
        let unknown = save(&store, "unknown", 150_000).await;
        make_idle(&store, on_other.id).await;
        make_idle(&store, unknown.id).await;
        set_turn_model(&store, on_other.id, "openai:other").await;
        set_turn_model(&store, unknown.id, "openai:retired").await;

        let stub = SummaryStub::default();
        let summarizer = Summarizer {
            provider: &stub,
            models: &models,
            default: &default,
        };
        assert_eq!(&other, summarizer.model_for(&store, &on_other).await);
        assert_eq!(&default, summarizer.model_for(&store, &unknown).await);

        compact_idle(&store, summarizer).await;
        let mut used = stub.used.lock().expect("test lock").clone();
        used.sort();
        assert_eq!(used, ["openai:default", "openai:other"]);
    }

    #[tokio::test]
    async fn a_session_without_turns_uses_the_default_model() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let default: Model = "openai:default".parse().unwrap();
        let models = ModelChoices::only(default.clone());
        let stub = SummaryStub::default();
        let summarizer = Summarizer {
            provider: &stub,
            models: &models,
            default: &default,
        };
        let session = SessionRef {
            user: "alice".into(),
            id: Uuid::new_v4(),
        };
        assert_eq!(&default, summarizer.model_for(&store, &session).await);
    }
}
