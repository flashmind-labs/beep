use super::*;
use crate::account::user::User;
use crate::store::{CreateRun, FinishedTurn, RunStatus, TestDb, test_chat, test_steps};
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
