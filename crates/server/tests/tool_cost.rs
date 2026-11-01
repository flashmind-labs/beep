//! What the built-in tools add to each model request. Runs a chat turn
//! and a routine turn for an admin with every optional built-in a test
//! server has, and prints the tools the model was offered, largest first:
//!
//!     just test-db
//!     BEEP_TEST_DATABASE_URL=... cargo test -p beep-server --test tool_cost -- --nocapture

use async_trait::async_trait;
use beep_server::{
    app::{App, Draws, Message, ServerTools},
    decision::DecisionTool,
    models::ModelChoices,
    protocol::DeviceKind,
    search::SearchTool,
    store::{CreateRun, TestDb},
    tool_cost::{Cost, Costs, size},
    tools::Transitous,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider,
    StreamEvent, ToolDefinition,
};
use iroh::SecretKey;
use secrecy::SecretString;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// Answers every request, keeping the tools of the last one that had any.
#[derive(Default)]
struct Recorder {
    tools: Mutex<Vec<ToolDefinition>>,
}

#[async_trait]
impl LlmProvider for Recorder {
    fn name(&self) -> &str {
        "test"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        if !request.tools.is_empty() {
            *self.tools.lock().expect("never poisoned") = request.tools.clone();
        }
        Box::pin(async_stream::stream! {
            yield Ok(StreamEvent::ContentDelta("Done.".into()));
            yield Ok(StreamEvent::Finished(FinishReason::Stop));
        })
    }
}

fn message(cron_id: Option<String>) -> Message {
    Message {
        session_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        message: "Hello".into(),
        scheduled: cron_id.as_ref().map(|_| "Check".to_owned()),
        cron_id,
        always_sent: false,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        voice: None,
        spoken: false,
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Phone {},
        platform: None,
        draws: Draws {
            charts: true,
            cards: true,
            sources: true,
        },
        finished_research: false,
    }
}

/// Every optional tool that only needs a key, with keys nothing accepts.
/// The sandbox's tools are left out: they need its runtime installed.
fn server_tools() -> ServerTools {
    ServerTools {
        search: Some(Arc::new(
            SearchTool::new("test-key".into()).expect("a well-formed key"),
        )),
        firecrawl_key: Some(SecretString::from("test-key")),
        decision: Some(Arc::new(
            DecisionTool::new(SecretString::from("test-key")).expect("a well-formed key"),
        )),
        transitous: Some(Arc::new(Transitous::new().expect("a client is made"))),
        ..ServerTools::default()
    }
}

/// The tools the model is offered in a turn of `message`.
async fn offered(message: Message) -> Vec<ToolDefinition> {
    let db = TestDb::new().await.expect("the test database is up");
    let store = db.store().await.expect("the test schema is made");
    let admin = SecretKey::generate();
    let provider = Arc::new(Recorder::default());
    let model = "openai:test".parse().expect("a valid model");
    let models = ModelChoices::resolve(&*provider, model, Vec::new(), &[admin.public()]).await;
    let app = App::new(
        store,
        provider.clone(),
        AgentLlmConfig::new("openai:test".parse().expect("a valid model")),
        server_tools(),
        models,
    );
    let user = app
        .sign_in(&admin.public())
        .await
        .expect("signing in works")
        .expect("a new key is never revoked")
        .user;
    let CreateRun::Created(_) = app.submit(&user, message).await.expect("submitted") else {
        panic!("the run was not created");
    };
    timeout(Duration::from_secs(20), async {
        loop {
            let tools = provider.tools.lock().expect("never poisoned").clone();
            if !tools.is_empty() {
                return tools;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the model is asked")
}

fn report(kind: &str, mut tools: Vec<ToolDefinition>) -> Costs {
    let costs = Costs::of(&tools);
    tools.sort_by_key(|tool| std::cmp::Reverse(size(tool)));
    println!("== {kind}: {costs}");
    for tool in &tools {
        let cost = Cost {
            tools: 1,
            bytes: size(tool),
        };
        println!(
            "{:>6} B ~{:>5} tok  {}",
            cost.bytes,
            cost.tokens(),
            tool.name
        );
    }
    costs
}

#[tokio::test]
async fn prints_what_built_in_tools_cost() {
    let chat = report("coordinator", offered(message(None)).await);
    let routine = report(
        "routine",
        offered(message(Some(Uuid::new_v4().to_string()))).await,
    );
    assert_eq!(chat.apps().tools, 0, "a test user has no apps");
    assert!(chat.built_in.tools > 0);
    assert!(routine.built_in.tools > 0);
}
