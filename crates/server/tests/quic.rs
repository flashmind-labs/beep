use async_trait::async_trait;
use beep_server::{
    app::{App, ServerTools},
    client::{Client, Replies},
    config::Auth,
    protocol::{
        ApiError, AppKind, CancelRun, Chat, ConnectApp, Event, GetEvents, GetRun, GetSession,
        Health, HistoryEntry, ListApps, MemorySource, RunEvent, StartAppAuth, SuggestPrompts,
    },
    starters,
    store::{CreateRun, Fact, Store},
    transport,
    user::User,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role,
    StreamEvent,
};
use iroh::SecretKey;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

const ALICE: &str = "alice-token-0123456789abcdef0123456789";

struct ProviderStub {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmProvider for ProviderStub {
    fn name(&self) -> &str {
        "test"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        // Memory learning and chat titles run in the background; they are not counted.
        let system = &request.messages[0].content;
        if system.starts_with("You keep the long-term memory") {
            return Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta(r#"{"ops":[]}"#.into()));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
            });
        }
        if system.starts_with("You name chats") {
            return Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta("Tea preference".into()));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
            });
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        let is_tool_result = request
            .messages
            .last()
            .is_some_and(|m| m.role == Role::Tool);
        let slow = request
            .messages
            .iter()
            .any(|m| m.role == Role::User && m.content == "slow");
        Box::pin(async_stream::stream! {
            if slow {
                yield Ok(StreamEvent::ContentDelta("Working".into()));
                tokio::time::sleep(Duration::from_secs(60)).await;
            } else if !is_tool_result {
                yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name: "memory_store".into() });
                yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: json!({"content":"Prefers tea"}).to_string() });
                yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
                return;
            }
            for text in ["I", " remember", " your", " preference", "."] {
                tokio::time::sleep(Duration::from_millis(20)).await;
                yield Ok(StreamEvent::ContentDelta(text.into()));
            }
            yield Ok(StreamEvent::Finished(FinishReason::Stop));
        })
    }
}

struct Harness {
    dir: tempfile::TempDir,
    router: Router,
    app: Arc<App>,
    provider: Arc<ProviderStub>,
}

impl Harness {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_for_test(dir.path().join("test.db"))
            .await
            .unwrap();
        let auth = Auth::new(vec![("alice".into(), ALICE.into())]).unwrap();
        let provider = Arc::new(ProviderStub {
            calls: AtomicUsize::new(0),
        });
        let app = App::new(
            store,
            auth,
            provider.clone(),
            AgentLlmConfig::new("openai:test".parse().unwrap()),
            ServerTools::default(),
        );
        let endpoint = iroh::Endpoint::builder(presets::N0)
            .secret_key(SecretKey::generate())
            .bind()
            .await
            .unwrap();
        let router = Router::builder(endpoint)
            .accept(transport::ALPN, transport::BeepHandler::new(app.clone()))
            .spawn();
        Self {
            dir,
            router,
            app,
            provider,
        }
    }

    async fn client(&self) -> Client {
        Client::connect(self.router.endpoint().addr(), ALICE)
            .await
            .unwrap()
    }
}

fn api_error(err: anyhow::Error) -> ApiError {
    err.downcast::<ApiError>().unwrap()
}

/// Reads events until the stream ends.
async fn drain(replies: &mut Replies<Event>) -> Vec<RunEvent> {
    let mut events = Vec::new();
    while let Some(event) = replies.next().await.unwrap() {
        events.push(event.event);
    }
    events
}

/// The run id from a stream's `RunStarted` event.
async fn started(replies: &mut Replies<Event>) -> String {
    match replies.next().await.unwrap().map(|e| e.event) {
        Some(RunEvent::RunStarted { run_id, .. }) => run_id,
        other => panic!("expected RunStarted, got {other:?}"),
    }
}

#[tokio::test]
async fn real_quic_stream_tools_reconnect_idempotency_and_user_isolation() {
    let h = Harness::new().await;
    let c = h.client().await;

    c.call(&Health {}).await.unwrap();
    let Err(rejected) = Client::connect(h.router.endpoint().addr(), "wrong-token").await else {
        panic!("wrong token accepted");
    };
    assert_eq!(api_error(rejected), ApiError::Unauthorized {});

    let session = Uuid::new_v4();
    let chat = Chat {
        session_id: session,
        request_id: Uuid::new_v4(),
        message: "Remember I prefer tea".into(),
        image: None,
        foreground: None,
    };
    let mut replies = c.stream(&chat).await.unwrap();
    let run_id = started(&mut replies).await;
    let evts = drain(&mut replies).await;
    assert!(matches!(evts.last(), Some(RunEvent::RunFinished { .. })));
    assert!(
        evts.iter()
            .any(|e| matches!(e, RunEvent::ToolResult { success: true, .. }))
    );
    let text: String = evts
        .iter()
        .filter_map(|e| match e {
            RunEvent::TextDelta { delta } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "I remember your preference.");

    drop(c);
    let c = h.client().await;

    // Resending the request follows the finished run instead of starting another.
    let mut again = c.stream(&chat).await.unwrap();
    assert!(matches!(
        drain(&mut again).await.as_slice(),
        [RunEvent::RunFinished { status }] if status == "completed"
    ));
    let mut resumed = c.stream(&GetEvents { run_id, after: 3 }).await.unwrap();
    assert!(matches!(
        drain(&mut resumed).await.as_slice(),
        [RunEvent::RunFinished { status }] if status == "completed"
    ));

    let entries = c
        .call(&GetSession {
            id: session.to_string(),
        })
        .await
        .unwrap();
    // One turn: the user message, then the final reply carrying the turn's tool result.
    let [
        HistoryEntry::User { content: asked, .. },
        HistoryEntry::Assistant { content, tools, .. },
    ] = entries.as_slice()
    else {
        panic!("unexpected history: {entries:?}");
    };
    assert_eq!(asked, "Remember I prefer tea");
    assert_eq!(content, "I remember your preference.");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "memory_store");
    assert!(tools[0].success && !tools[0].call_id.is_empty() && !tools[0].output.is_empty());

    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 2);

    let reopened = Store::open_for_test(h.dir.path().join("test.db"))
        .await
        .unwrap();
    assert_eq!(
        User::for_test(reopened.clone(), "alice")
            .memory_rows()
            .await
            .unwrap()[0]
            .content,
        "Prefers tea"
    );
    assert!(
        User::for_test(reopened.clone(), "alice")
            .conversation(&session.to_string())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn cancellation_busy_session_and_invalid_input() {
    let h = Harness::new().await;
    let c = h.client().await;

    let session_id = Uuid::new_v4();
    let slow = Chat {
        session_id,
        request_id: Uuid::new_v4(),
        message: "slow".into(),
        image: None,
        foreground: None,
    };
    let mut slow_events = c.stream(&slow).await.unwrap();
    let slow_run = started(&mut slow_events).await;

    let busy = Chat {
        session_id,
        request_id: Uuid::new_v4(),
        message: "again".into(),
        image: None,
        foreground: None,
    };
    let mut busy = c.stream(&busy).await.unwrap();
    assert!(matches!(
        api_error(busy.next().await.unwrap_err()),
        ApiError::Conflict { .. }
    ));

    let empty = Chat {
        session_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        message: " ".into(),
        image: None,
        foreground: None,
    };
    let mut empty = c.stream(&empty).await.unwrap();
    assert!(matches!(
        api_error(empty.next().await.unwrap_err()),
        ApiError::BadRequest { .. }
    ));

    let missing = GetRun {
        id: "missing".into(),
    };
    assert_eq!(
        api_error(c.call(&missing).await.unwrap_err()),
        ApiError::NotFound {}
    );
    let mut events = c
        .stream(&GetEvents {
            run_id: "missing".into(),
            after: 0,
        })
        .await
        .unwrap();
    assert_eq!(
        api_error(events.next().await.unwrap_err()),
        ApiError::NotFound {}
    );

    tokio::time::sleep(Duration::from_millis(200)).await;
    c.call(&CancelRun { id: slow_run }).await.unwrap();

    let evts = drain(&mut slow_events).await;
    assert!(
        evts.iter()
            .any(|e| matches!(e, RunEvent::RunFinished { status } if status == "cancelled"))
    );

    h.app.shutdown().await;
}

#[tokio::test]
async fn crash_recovery_and_memory_updates_are_durable_and_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let store = Store::open_for_test(&path).await.unwrap();
    let run = match User::for_test(store.clone(), "alice")
        .create_run("session", "request", "hello")
        .await
        .unwrap()
    {
        CreateRun::Created(run) => run,
        _ => panic!("expected new run"),
    };
    let memory = |content| Fact {
        content,
        category: None,
        source: MemorySource::User {},
        valid: None,
    };
    let coffee = User::for_test(store.clone(), "alice")
        .add_memory(memory("coffee"))
        .await
        .unwrap();
    User::for_test(store.clone(), "alice")
        .add_memory(memory("tea"))
        .await
        .unwrap();
    User::for_test(store.clone(), "bob")
        .add_memory(memory("water"))
        .await
        .unwrap();
    assert!(
        !User::for_test(store.clone(), "bob")
            .forget("unknown")
            .await
            .unwrap()
    );
    assert!(
        !User::for_test(store.clone(), "bob")
            .forget(&coffee)
            .await
            .unwrap()
    );
    assert!(
        User::for_test(store.clone(), "alice")
            .forget(&coffee)
            .await
            .unwrap()
    );
    assert_eq!(
        User::for_test(store.clone(), "alice")
            .memory_rows()
            .await
            .unwrap()[0]
            .content,
        "tea"
    );
    assert_eq!(
        User::for_test(store.clone(), "bob")
            .memory_rows()
            .await
            .unwrap()[0]
            .content,
        "water"
    );
    drop(store);
    let store = Store::open_for_test(&path).await.unwrap();
    store.recover().await.unwrap();
    store.recover().await.unwrap();
    assert_eq!(
        User::for_test(store.clone(), "alice")
            .run(&run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "interrupted"
    );
    assert!(matches!(
        User::for_test(store.clone(), "alice")
            .create_run("session", "new-request", "hello")
            .await
            .unwrap(),
        CreateRun::Created(_)
    ));
}

#[tokio::test]
async fn prompt_suggestions_fall_back_and_apps_list_without_composio() {
    let h = Harness::new().await;
    let c = h.client().await;

    // The stub answers with a tool call instead of JSON, so the fixed starters come back.
    let suggestions = c.call(&SuggestPrompts { refresh: false }).await.unwrap();
    assert_eq!(suggestions.suggestions, starters::fallback());
    assert!(!suggestions.personal);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    // Cached: no second model call.
    let cached = c.call(&SuggestPrompts { refresh: false }).await.unwrap();
    assert_eq!(cached, suggestions);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    // A refresh asks the model again.
    c.call(&SuggestPrompts { refresh: true }).await.unwrap();
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 2);

    let apps = c.call(&ListApps {}).await.unwrap();
    assert_eq!(apps.len(), 9);
    assert_eq!(apps[0].slug, "gmail");
    assert_eq!(apps[8].kind, AppKind::Fastmail {});
    assert!(apps.iter().all(|a| !a.connected));
    // Composio needs a server key; Fastmail is connected per user with OAuth.
    assert!(
        apps.iter()
            .all(|a| a.configured == (a.kind == AppKind::Fastmail {}))
    );

    let error = c
        .call(&StartAppAuth {
            slug: "fastmail".into(),
            redirect_uri: "https://example.com/callback".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(api_error(error), ApiError::BadRequest { .. }));

    let error = c
        .call(&ConnectApp {
            slug: "gmail".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(api_error(error), ApiError::BadRequest { .. }));
}
