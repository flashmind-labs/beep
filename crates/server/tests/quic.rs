use async_trait::async_trait;
use bytes::Bytes;
use flashmind::types::{
    CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role, StreamEvent,
};
use http::StatusCode;
use neobeep_server::{
    app::App,
    client::Client,
    config::Auth,
    store::{CreateRun, Store},
    transport,
};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
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
    cert: CertificateDer<'static>,
    endpoint: quinn::Endpoint,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
    web_task: tokio::task::JoinHandle<anyhow::Result<()>>,
    web_port: u16,
    app: Arc<App>,
    provider: Arc<ProviderStub>,
}

impl Harness {
    async fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
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
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = cert.der().clone();
        let endpoint = transport::endpoint(
            "127.0.0.1:0".parse().unwrap(),
            vec![cert.clone()],
            PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
        let task = tokio::spawn(transport::serve(endpoint.clone(), app.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let web_port = listener.local_addr().unwrap().port();
        let web_task = tokio::spawn(neobeep_server::web::serve(listener, app.clone()));
        Self {
            dir,
            cert,
            endpoint,
            task,
            web_task,
            web_port,
            app,
            provider,
        }
    }
    async fn client(&self) -> Client {
        Client::connect(
            self.endpoint.local_addr().unwrap(),
            "localhost",
            vec![self.cert.clone()],
        )
        .await
        .unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.endpoint.close(0u32.into(), b"test finished");
        self.task.abort();
        self.web_task.abort();
    }
}

async fn request(
    client: &mut Client,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, String) {
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
    let mut client = h.client().await;
    assert_eq!(
        request(&mut client, "GET", "/v1/health", "wrong", None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, health) = request(&mut client, "GET", "/v1/health", ALICE, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<Value>(&health).unwrap()["protocol"],
        "h3"
    );
    let session = Uuid::new_v4();
    let body =
        json!({"session_id":session,"request_id":Uuid::new_v4(),"message":"Remember I prefer tea"});
    let (status, raw) = request(&mut client, "POST", "/v1/runs", ALICE, Some(body.clone())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run: Value = serde_json::from_str(&raw).unwrap();
    let id = run["id"].as_str().unwrap();
    // Closing the transport must not cancel server-owned execution.
    drop(client);
    let mut client = h.client().await;
    let (status, duplicate) =
        request(&mut client, "POST", "/v1/runs", ALICE, Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<Value>(&duplicate).unwrap()["id"],
        run["id"]
    );
    let mut changed = body.clone();
    changed["message"] = json!("different message");
    assert_eq!(
        request(&mut client, "POST", "/v1/runs", ALICE, Some(changed))
            .await
            .0,
        StatusCode::CONFLICT
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
        assert_eq!(
            request(&mut client, method, &path, BOB, None).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (status, raw) = request(
        &mut client,
        "GET",
        &format!("/v1/runs/{id}/events"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
        &mut client,
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
        &mut client,
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
            &mut client,
            "GET",
            &format!("/v1/sessions/{session}"),
            BOB,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        h.app
            .store
            .recall("bob", "")
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Reopen the SQLite file to prove memory, conversation, and replay survive storage reopen.
    let reopened = Store::open(h.dir.path().join("test.db")).await.unwrap();
    assert_eq!(
        reopened.recall("alice", "tea").await.unwrap()[0]["content"],
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
    let mut client = h.client().await;
    assert_eq!(
        request(
            &mut client,
            "POST",
            "/v1/runs",
            ALICE,
            Some(json!({"message":"missing ids"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let body = json!({"session_id":Uuid::new_v4(),"request_id":Uuid::new_v4(),"message":"slow"});
    let (_, raw) = request(&mut client, "POST", "/v1/runs", ALICE, Some(body.clone())).await;
    let run: Value = serde_json::from_str(&raw).unwrap();
    let id = run["id"].as_str().unwrap();
    let mut concurrent = body.clone();
    concurrent["request_id"] = json!(Uuid::new_v4());
    assert_eq!(
        request(&mut client, "POST", "/v1/runs", ALICE, Some(concurrent))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &mut client,
            "GET",
            &format!("/v1/runs/{id}/events?after=-1"),
            ALICE,
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut subscriber = h.client().await;
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
    // Observe streamed text while generation is still blocked, before cancelling it.
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
            &mut client,
            "POST",
            &format!("/v1/runs/{id}/cancel"),
            ALICE,
            None
        )
        .await
        .0,
        StatusCode::ACCEPTED
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
    store.remember("alice", "drink", "coffee").await.unwrap();
    store.remember("alice", "drink", "tea").await.unwrap();
    store.remember("bob", "drink", "water").await.unwrap();
    assert!(!store.forget("bob", "unknown").await.unwrap());
    assert!(store.forget("alice", "drink").await.unwrap());
    assert_eq!(
        store.recall("bob", "").await.unwrap()[0]["content"],
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

#[tokio::test]
async fn quic_rejects_untrusted_server_certificates() {
    let h = Harness::new().await;
    let wrong_ca = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let result = Client::connect(
        h.endpoint.local_addr().unwrap(),
        "localhost",
        vec![wrong_ca.cert.der().clone()],
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn browser_and_android_share_authenticated_api() {
    let h = Harness::new().await;
    let browser = reqwest::Client::new();
    let base = format!("http://localhost:{}", h.web_port);
    let page = browser.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert!(page.headers().contains_key("content-security-policy"));
    assert!(page.text().await.unwrap().contains("Connect to your Beep"));
    for path in [
        "/.dev/android-app-token",
        "/config.toml",
        "/protocol.test.mjs",
    ] {
        assert_eq!(
            browser
                .get(format!("{base}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        browser
            .get(format!("{base}/v1/sessions"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let session = Uuid::new_v4();
    let body =
        json!({"session_id":session,"request_id":Uuid::new_v4(),"message":"Hello from browser"});
    let response = browser
        .post(format!("{base}/v1/runs"))
        .bearer_auth(ALICE)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let run: Value = response.json().await.unwrap();
    let events = browser
        .get(format!(
            "{base}/v1/runs/{}/events",
            run["id"].as_str().unwrap()
        ))
        .bearer_auth(ALICE)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(events.contains("run_finished"));
    assert!(events.contains("text_delta"));
    let mut android = h.client().await;
    let (status, conversation) = request(
        &mut android,
        "GET",
        &format!("/v1/sessions/{session}"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(conversation.contains("Hello from browser"));
    let listed: Value = browser
        .get(format!("{base}/v1/sessions"))
        .bearer_auth(ALICE)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed["sessions"][0]["id"], session.to_string());
    let other: Value = browser
        .get(format!("{base}/v1/sessions"))
        .bearer_auth(BOB)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(other["sessions"].as_array().unwrap().len(), 0);
    assert_eq!(
        browser
            .get(format!("{base}/v1/sessions/{session}"))
            .bearer_auth(BOB)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}
