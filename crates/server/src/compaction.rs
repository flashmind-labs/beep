//! Shrinks the model's context of long sessions, so later turns send less
//! history. Right after a turn whose prompt reached `compact_at`, and again
//! for any session left over it once it goes quiet, old tool output is
//! dropped first; if that is not enough, the older turns are summarized.
//! Only the model's context is compacted; the transcript the phone shows
//! keeps every message.

use crate::{
    app::App,
    metering, permissions,
    store::{SessionRef, Store},
};
use anyhow::Result;
use flashmind::{
    core::{Conversation, EntryKind},
    types::{LlmProvider, Model},
};
use std::{sync::Arc, time::Duration};

/// The sweep compacts a session over `compact_at` that nobody has used for
/// this long, in case compacting right after its turn lost to a newer run.
pub const IDLE_BEFORE_COMPACT: Duration = Duration::from_secs(15 * 60);
/// Prompt size that triggers compaction. Past it models lose the thread and
/// every turn costs more, whatever their window.
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

/// The model and provider that write the summary.
pub struct Summarizer<'a> {
    pub provider: &'a dyn LlmProvider,
    pub model: &'a Model,
}

/// Token limit for a model's context window: 250k, or 80% of a smaller
/// window so the summary runs before the window fills mid-turn.
pub fn compact_at(window: Option<u32>) -> u32 {
    LIMIT.min(window.unwrap_or(DEFAULT_WINDOW) / 5 * 4)
}

/// Compacts every session that is both idle and over `limit` tokens, the
/// chat model's `compact_at`.
pub async fn compact_idle(store: &Store, summarizer: Summarizer<'_>, limit: u32) {
    let sessions = match store.sessions_to_compact(limit, IDLE_BEFORE_COMPACT).await {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("unable to list sessions to compact: {error}");
            return;
        }
    };
    for session in sessions {
        compact_session(store, &summarizer, &session, limit).await;
    }
}

/// Compacts one session and logs how it went.
pub async fn compact_session(
    store: &Store,
    summarizer: &Summarizer<'_>,
    session: &SessionRef,
    limit: u32,
) {
    match compact(store, summarizer, session, limit).await {
        Ok(true) => log::info!("session compacted; session={}", session.id),
        Ok(false) => {}
        Err(error) => log::warn!("compaction failed: {error}; session={}", session.id),
    }
}

/// Returns whether the session's context was replaced.
async fn compact(
    store: &Store,
    summarizer: &Summarizer<'_>,
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
        context.compact_with_llm_keeping_tokens(
            summarizer.provider,
            summarizer.model,
            KEEP_RECENT_TOKENS,
        ),
    )
    .await?;
    if summary.is_none() {
        // Nothing old enough to summarize. Saving still resets the token count,
        // so the session is not picked again until a new turn measures it.
        // Keeps whatever tool output was trimmed above.
        store.save_compacted(session, &before, &context).await?;
        return Ok(false);
    }
    // A run that started meanwhile changed the context; the next idle sweep retries.
    store.save_compacted(session, &before, &context).await
}

/// Replaces long tool output older than the last `keep_recent_tokens` with a
/// stub saying how much was there. The calls and the replies that used them
/// stay. Returns the characters dropped.
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
    use uuid::Uuid;

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
    async fn compacts_only_long_idle_sessions_and_keeps_the_messages() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let model: Model = "openai:test".parse().unwrap();
        let summarizer = || Summarizer {
            provider: &SummaryStub,
            model: &model,
        };

        let long_idle = save(&store, "long-idle", 150_000).await;
        let long_active = save(&store, "long-active", 150_000).await;
        let short_idle = save(&store, "short-idle", 50_000).await;
        make_idle(&store, long_idle.id).await;
        make_idle(&store, short_idle.id).await;

        compact_idle(&store, summarizer(), compact_at(None)).await;

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
        let summarizer = Summarizer {
            provider: &SummaryStub,
            model: &model,
        };
        let mut chat = Conversation::with_system("system");
        chat.add(ConversationEntry::user("Find the bank emails"));
        chat.add(ConversationEntry::tool("old", "x".repeat(300_000)));
        chat.add(ConversationEntry::tool("new", "y".repeat(70_000)));
        let session = save_chat(&store, "one-message", &chat, 150_000).await;

        // Still over half the limit once trimmed, and one user message
        // leaves nothing to summarize.
        assert!(
            !compact(&store, &summarizer, &session, 40_000)
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
}
