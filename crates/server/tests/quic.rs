use async_trait::async_trait;
use beep_server::{
    app::App,
    client::Client,
    config::Auth,
    protocol::{HistoryEntry, Reply, Request, Run, RunEvent},
    store::{CreateRun, Store},
    transport::{self, RemoteError},
};
use flashmind::types::{
    CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role, StreamEvent,
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
                yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: json!({"key":"drink","content":"Prefers tea"}).to_string() });
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
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        let auth = Auth::new(vec![("alice".into(), ALICE.into())]).unwrap();
        let provider = Arc::new(ProviderStub {
            calls: AtomicUsize::new(0),
        });
        let app = App::new(
            store,
            auth,
            provider.clone(),
            "openai:test".parse().unwrap(),
            None,
            None,
            None,
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

async fn run(c: &Client, request: &Request) -> anyhow::Result<Run> {
    match c.call(request).await? {
        Reply::Run { run } => Ok(run),
        other => anyhow::bail!("expected run, got {other:?}"),
    }
}

async fn done(c: &Client, request: &Request) -> anyhow::Result<()> {
    match c.call(request).await? {
        Reply::Done {} => Ok(()),
        other => anyhow::bail!("expected done, got {other:?}"),
    }
}

fn code(err: anyhow::Error) -> u16 {
    err.downcast_ref::<RemoteError>().unwrap().code
}

/// Polls the event stream until the run reports `RunFinished`.
async fn finished_events(c: &Client, run_id: &str) -> Vec<RunEvent> {
    let mut events = Vec::new();
    for _ in 0..50 {
        events.clear();
        let mut replies = c
            .stream(&Request::GetEvents {
                run_id: run_id.into(),
                after: 0,
            })
            .await
            .unwrap();
        while let Some(frame) = replies.next().await.unwrap() {
            let Reply::Event { event, .. } = frame else {
                panic!("expected event, got {frame:?}")
            };
            events.push(event);
        }
        if events
            .iter()
            .any(|e| matches!(e, RunEvent::RunFinished { .. }))
        {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("run {run_id} never finished");
}

#[tokio::test]
async fn real_quic_stream_tools_reconnect_idempotency_and_user_isolation() {
    let h = Harness::new().await;
    let c = h.client().await;

    done(&c, &Request::Health {}).await.unwrap();
    assert!(
        Client::connect(h.router.endpoint().addr(), "wrong-token")
            .await
            .is_err()
    );

    let session = Uuid::new_v4();
    let chat = Request::Chat {
        session_id: session,
        request_id: Uuid::new_v4(),
        message: "Remember I prefer tea".into(),
        image: None,
    };
    let first = run(&c, &chat).await.unwrap();

    drop(c);
    let c = h.client().await;

    let again = run(&c, &chat).await.unwrap();
    assert_eq!(again.id, first.id);

    let evts = finished_events(&c, &first.id).await;
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

    let Reply::History { entries } = c
        .call(&Request::GetSession {
            id: session.to_string(),
        })
        .await
        .unwrap()
    else {
        panic!("expected history")
    };
    let contents: Vec<&str> = entries
        .iter()
        .map(|e| match e {
            HistoryEntry::User { content, .. } | HistoryEntry::Assistant { content, .. } => {
                content.as_str()
            }
        })
        .collect();
    assert!(!contents.iter().any(|c| c.contains("You are Beep")));
    assert!(contents.contains(&"I remember your preference."));

    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 2);

    let reopened = Store::open(h.dir.path().join("test.db")).await.unwrap();
    assert_eq!(
        reopened.recall("alice", "tea", None).await.unwrap()[0]["content"],
        "Prefers tea"
    );
    assert!(
        reopened
            .conversation("alice", &session.to_string())
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
    let slow = Request::Chat {
        session_id,
        request_id: Uuid::new_v4(),
        message: "slow".into(),
        image: None,
    };
    let slow_run = run(&c, &slow).await.unwrap();

    let busy = Request::Chat {
        session_id,
        request_id: Uuid::new_v4(),
        message: "again".into(),
        image: None,
    };
    assert_eq!(code(c.call(&busy).await.unwrap_err()), 409);

    let empty = Request::Chat {
        session_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        message: " ".into(),
        image: None,
    };
    assert_eq!(code(c.call(&empty).await.unwrap_err()), 400);

    let missing = Request::GetRun {
        id: "missing".into(),
    };
    assert_eq!(code(c.call(&missing).await.unwrap_err()), 404);

    tokio::time::sleep(Duration::from_millis(200)).await;
    let cancel = Request::CancelRun {
        id: slow_run.id.clone(),
    };
    done(&c, &cancel).await.unwrap();

    let evts = finished_events(&c, &slow_run.id).await;
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
    let store = Store::open(&path).await.unwrap();
    let run = match store
        .create_run("alice", "session", "request", "hello")
        .await
        .unwrap()
    {
        CreateRun::Created(run) => run,
        _ => panic!("expected new run"),
    };
    store
        .remember("alice", "drink", "coffee", None)
        .await
        .unwrap();
    store.remember("alice", "drink", "tea", None).await.unwrap();
    store.remember("bob", "drink", "water", None).await.unwrap();
    assert!(!store.forget("bob", "unknown").await.unwrap());
    assert!(store.forget("alice", "drink").await.unwrap());
    assert_eq!(
        store.recall("bob", "", None).await.unwrap()[0]["content"],
        "water"
    );
    drop(store);
    let store = Store::open(&path).await.unwrap();
    store.recover().await.unwrap();
    store.recover().await.unwrap();
    assert_eq!(
        store.run("alice", &run.id).await.unwrap().unwrap().status,
        "interrupted"
    );
    assert_eq!(
        store.events(&run.id, 0).await.unwrap().len(),
        2,
        "recovery must be idempotent"
    );
    assert!(matches!(
        store
            .create_run("alice", "session", "new-request", "hello")
            .await
            .unwrap(),
        CreateRun::Created(_)
    ));
}
