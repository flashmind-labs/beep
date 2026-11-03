//! A two-user app for watcher tests, on a scripted model.

use crate::{
    app::{App, ServerTools},
    models::ModelChoices,
    store::{Account, TestDb, TestTurn, test_turn_with},
    user::{User, UserKey},
};
use async_trait::async_trait;
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider,
    StreamEvent,
};

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicI64, Ordering},
};
use uuid::Uuid;

static NEXT: AtomicI64 = AtomicI64::new(0);

/// Answers with the first rule whose text starts the system prompt, and records every prompt.
#[derive(Default)]
pub(crate) struct Scripted {
    rules: Vec<(&'static str, String)>,
    pub prompts: Mutex<Vec<String>>,
}

impl Scripted {
    pub fn answer(mut self, system_starts: &'static str, reply: &str) -> Self {
        self.rules.push((system_starts, reply.to_owned()));
        self
    }

    pub fn prompts(&self) -> Vec<String> {
        self.prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl LlmProvider for Scripted {
    fn name(&self) -> &str {
        "test"
    }

    fn provider(&self) -> Provider {
        Provider::OpenAi
    }

    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        let system = request.messages[0].content.clone();
        if let Some(prompt) = request.messages.get(1) {
            self.prompts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(prompt.content.clone());
        }
        let reply = self
            .rules
            .iter()
            .find(|(starts, _)| system.starts_with(starts))
            .map_or_else(|| "{}".to_owned(), |(_, reply)| reply.clone());
        Box::pin(async_stream::stream! {
            yield Ok(StreamEvent::ContentDelta(reply));
            yield Ok(StreamEvent::Finished(FinishReason::Stop));
        })
    }
}

pub(crate) struct Harness {
    pub _db: TestDb,
    pub app: Arc<App>,
    pub model: Arc<Scripted>,
    pub alice: Arc<User>,
    pub bob: Arc<User>,
}

impl Harness {
    pub async fn new(model: Scripted) -> Self {
        let db = TestDb::new().await.expect("a test database");
        let store = db.store().await.expect("a store");
        let model = Arc::new(model);
        let default = "openai:test".parse().expect("a model name");
        let models = ModelChoices::resolve(model.as_ref(), default, Vec::new()).await;
        let app = App::new(
            store,
            model.clone(),
            AgentLlmConfig::new("openai:test".parse().expect("a model name")),
            ServerTools::default(),
            models,
        );
        let start = |id: &str| {
            app.user_for(&Account {
                id: id.to_owned(),
                external_id: id.to_owned(),
            })
        };
        let (alice, bob) = (start("alice"), start("bob"));
        Self {
            _db: db,
            app,
            model,
            alice,
            bob,
        }
    }

    /// A turn of `user`'s chat sent now. Returns the id of its last message.
    pub async fn turn(&self, user: &User, chat: Uuid, prompt: &str) -> i64 {
        self.turn_with_calls(user, chat, prompt, 0).await
    }

    /// [`Self::turn`] with `calls` tool calls before the reply.
    pub async fn turn_with_calls(
        &self,
        user: &User,
        chat: Uuid,
        prompt: &str,
        calls: usize,
    ) -> i64 {
        // Apart by a few milliseconds, so each turn has its own request id.
        let at = chrono::Utc::now().timestamp_millis() + NEXT.fetch_add(10, Ordering::Relaxed);
        let turn = TestTurn { prompt, at, calls };
        test_turn_with(&self.app.store, &UserKey::for_test(user.id()), chat, turn)
            .await
            .expect("a saved turn");
        self.app
            .store
            .watched_chat(user.id(), chat)
            .await
            .expect("the chat reads")
            .expect("the chat is a session")
            .last_seq
    }
}
