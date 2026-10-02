use async_trait::async_trait;
use beep_server::{
    app::App,
    client::Client,
    config::Auth,
    store::{CreateRun, Store},
    transport,
};
use bytes::Bytes;
use flashmind::types::{
    CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role, StreamEvent,
};
use iroh::SecretKey;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

const ALICE: &str = "test-alice-token-with-at-least-32-characters";
const BOB: &str = "test-bob-token-with-at-least-32-characters";

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
                yield Ok(StreamEvent::ToolCallStart {index:0,id:Uuid::new_v4().to_string(),name:"memory_store".into()});
                yield Ok(StreamEvent::ToolCallDelta {index:0,arguments:json!({"key":"drink","content":"Prefers tea"}).to_string()});
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
        let auth = Auth::new(vec![
            ("alice".into(), ALICE.into()),
            ("bob".into(), BOB.into()),
        ])
        .unwrap();
        let provider = Arc::new(ProviderStub {
            calls: AtomicUsize::new(0),
        });
        let app = App::new(
            store,
            auth,
            provider.clone(),
            "openai:test".parse().unwrap(),
        );
        let endpoint = iroh::Endpoint::builder(presets::N0)
            .secret_key(SecretKey::generate())
            .bind()
            .await
            .unwrap();
        let handler = transport::BeepHandler::new(app.clone());
        let router = Router::builder(endpoint)
            .accept(transport::ALPN, handler)
            .spawn();
        Self {
            dir,
            router,
            app,
            provider,
        }
    }

    async fn client(&self) -> Client {
        let addr = self.router.endpoint().addr();
        Client::connect(addr).await.unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {}
}

async fn request(
    client: &Client,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, String) {
    let mut output = Vec::new();
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        client.request(
            method,
            path,
            token,
            body.map(|v| Bytes::from(v.to_string())),
            |bytes| output.extend_from_slice(bytes),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    (status, String::from_utf8(output).unwrap())
}

fn events(raw: &str) -> Vec<Value> {
    raw.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn real_quic_stream_tools_reconnect_idempotency_and_user_isolation() {
    let h = Harness::new().await;
    let client = h.client().await;
    assert_eq!(
        request(&client, "GET", "/v1/health", "wrong", None).await.0,
        401
    );
    let (status, health) = request(&client, "GET", "/v1/health", ALICE, None).await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&health).unwrap()["protocol"],
        "iroh"
    );
    let session = Uuid::new_v4();
    let body =
        json!({"session_id":session,"request_id":Uuid::new_v4(),"message":"Remember I prefer tea"});
    let (status, raw) = request(&client, "POST", "/v1/runs", ALICE, Some(body.clone())).await;
    assert_eq!(status, 202);
    let run: Value = serde_json::from_str(&raw).unwrap();
    let id = run["id"].as_str().unwrap();
    // Closing the transport must not cancel server-owned execution.
    drop(client);
    let client = h.client().await;
    let (status, duplicate) = request(&client, "POST", "/v1/runs", ALICE, Some(body.clone())).await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&duplicate).unwrap()["id"],
        run["id"]
    );
    let mut changed = body.clone();
    changed["message"] = json!("different message");
    assert_eq!(
        request(&client, "POST", "/v1/runs", ALICE, Some(changed))
            .await
            .0,
        409
    );
    for path in [
        format!("/v1/runs/{id}"),
        format!("/v1/runs/{id}/events"),
        format!("/v1/runs/{id}/cancel"),
    ] {
        let method = if path.ends_with("cancel") {
            "POST"
        } else {
            "GET"
        };
        assert_eq!(request(&client, method, &path, BOB, None).await.0, 404);
    }
    let (status, raw) = request(
        &client,
        "GET",
        &format!("/v1/runs/{id}/events"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let full = events(&raw);
    assert_eq!(full.last().unwrap()["status"], "completed");
    assert!(
        full.iter()
            .any(|e| e["type"] == "tool_result" && e["success"] == true)
    );
    assert_eq!(
        full.iter()
            .filter_map(|e| e["delta"].as_str())
            .collect::<String>(),
        "I remember your preference."
    );
    for pair in full.windows(2) {
        assert_eq!(
            pair[1]["seq"].as_i64().unwrap(),
            pair[0]["seq"].as_i64().unwrap() + 1
        );
    }
    let (_, replayed) = request(
        &client,
        "GET",
        &format!("/v1/runs/{id}/events?after=2"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(events(&replayed), full[2..]);
    assert_eq!(
        h.provider.calls.load(Ordering::SeqCst),
        2,
        "duplicate submission must not invoke provider again"
    );
    let (_, history) = request(
        &client,
        "GET",
        &format!("/v1/sessions/{session}"),
        ALICE,
        None,
    )
    .await;
    assert!(!history.contains("You are Beep"));
    assert!(!history.contains("system_prompt"));
    assert!(history.contains("I remember your preference."));
    assert_eq!(
        request(
            &client,
            "GET",
            &format!("/v1/sessions/{session}"),
            BOB,
            None
        )
        .await
        .0,
        404
    );
    assert!(
        h.app
            .store
            .recall("bob", "", None)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
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
    assert_eq!(reopened.events(id, 0).await.unwrap().len(), full.len());
}

#[tokio::test]
async fn cancellation_busy_session_and_invalid_input() {
    let h = Harness::new().await;
    let client = h.client().await;
    assert_eq!(
        request(
            &client,
            "POST",
            "/v1/runs",
            ALICE,
            Some(json!({"message":"missing ids"}))
        )
        .await
        .0,
        400
    );
    let body = json!({"session_id":Uuid::new_v4(),"request_id":Uuid::new_v4(),"message":"slow"});
    let (_, raw) = request(&client, "POST", "/v1/runs", ALICE, Some(body.clone())).await;
    let run: Value = serde_json::from_str(&raw).unwrap();
    let id = run["id"].as_str().unwrap();
    let mut concurrent = body.clone();
    concurrent["request_id"] = json!(Uuid::new_v4());
    assert_eq!(
        request(&client, "POST", "/v1/runs", ALICE, Some(concurrent))
            .await
            .0,
        409
    );
    assert_eq!(
        request(
            &client,
            "GET",
            &format!("/v1/runs/{id}/events?after=-1"),
            ALICE,
            None
        )
        .await
        .0,
        400
    );
    let subscriber = h.client().await;
    let events_path = format!("/v1/runs/{id}/events");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let subscription = tokio::spawn(async move {
        let mut output = Vec::new();
        subscriber
            .request("GET", &events_path, ALICE, None, |chunk| {
                output.extend_from_slice(chunk);
                let _ = tx.send(output.clone());
            })
            .await
            .unwrap();
        String::from_utf8(output).unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(bytes) = rx.recv().await {
            if String::from_utf8_lossy(&bytes).contains("Working") {
                return;
            }
        }
        panic!("stream ended before text arrived");
    })
    .await
    .unwrap();
    assert_eq!(
        request(
            &client,
            "POST",
            &format!("/v1/runs/{id}/cancel"),
            ALICE,
            None
        )
        .await
        .0,
        202
    );
    let raw = tokio::time::timeout(Duration::from_secs(5), subscription)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(events(&raw).last().unwrap()["status"], "cancelled");
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
