use crate::{
    app::{App, Draws, Message, ServerTools},
    models::ModelChoices,
    protocol::{DeviceKind, RunEvent, WebSource},
    search::SearchTool,
    store::{Account, TestDb},
};
use async_trait::async_trait;
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role,
    StreamEvent,
};
use iroh::SecretKey;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Semaphore,
    time::timeout,
};
use uuid::Uuid;

/// Searches once, then answers. The first search waits for a permit, so the
/// test can follow the run from its start.
struct Searcher {
    go: Arc<Semaphore>,
}

#[async_trait]
impl LlmProvider for Searcher {
    fn name(&self) -> &str {
        "test"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        let main = request.tools.iter().any(|tool| tool.name == "web_search");
        let after_tool = request
            .messages
            .last()
            .is_some_and(|message| message.role == Role::Tool);
        let go = self.go.clone();
        Box::pin(async_stream::stream! {
            if main && !after_tool {
                let _permit = go.acquire().await;
                yield Ok(StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call-1".into(),
                    name: "web_search".into(),
                });
                yield Ok(StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments: json!({"query": "halloween events tokyo"}).to_string(),
                });
                yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
            } else {
                let reply = if main { "There is a parade in Shibuya." } else { "{}" };
                yield Ok(StreamEvent::ContentDelta(reply.into()));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
            }
        })
    }
}

/// Answers one search with two results, the second twice.
async fn firecrawl(listener: TcpListener) {
    let (mut stream, _) = listener.accept().await.expect("the search connects");
    let mut data = Vec::new();
    while !data.ends_with(b"}") {
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.expect("the request reads");
        assert!(n > 0, "the request ended early");
        data.extend_from_slice(&buf[..n]);
    }
    let body = json!({"success": true, "data": {"web": [
        {"url": "https://www.tokyocheapo.com/events/halloween/", "title": "Halloween in Tokyo"},
        {"url": "https://www.gaijinpot.com/halloween", "title": "Shibuya Halloween"},
        {"url": "https://www.gaijinpot.com/halloween#map", "title": "Again"},
    ]}})
    .to_string();
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(reply.as_bytes())
        .await
        .expect("the reply writes");
}

fn message(session_id: Uuid) -> Message {
    Message {
        session_id,
        request_id: Uuid::new_v4(),
        message: "Halloween events today in Tokyo".into(),
        cron_id: None,
        scheduled: None,
        always_sent: false,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        voice: None,
        spoken: false,
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Desktop {},
        platform: None,
        draws: Draws {
            sources: true,
            ..Draws::default()
        },
        finished_research: false,
    }
}

#[tokio::test]
async fn a_reply_keeps_the_sources_of_its_web_searches() {
    let db = TestDb::new().await.expect("a test database");
    let store = db.store().await.expect("a store");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let endpoint = format!("http://{}/v2/search", listener.local_addr().expect("bound"));
    let server = tokio::spawn(firecrawl(listener));
    let go = Arc::new(Semaphore::new(0));
    let provider = Arc::new(Searcher { go: go.clone() });
    let admin = SecretKey::generate().public();
    let model = "openai:test".parse().expect("a model name");
    let models = ModelChoices::resolve(provider.as_ref(), model, Vec::new(), &[admin]).await;
    let tools = ServerTools {
        search: Some(Arc::new(SearchTool::at(endpoint).expect("a search tool"))),
        ..ServerTools::default()
    };
    let app = App::new(
        store,
        provider,
        AgentLlmConfig::new("openai:test".parse().expect("a model name")),
        tools,
        models,
    );
    let user = app.user_for(&Account {
        id: "alice".into(),
        external_id: "alice".into(),
    });
    let chat = Uuid::new_v4();
    let crate::store::CreateRun::Created(run) =
        app.submit(&user, message(chat)).await.expect("submitted")
    else {
        panic!("the run was not created");
    };
    let live = app
        .live(&run.id)
        .await
        .expect("the run waits for the model");
    let mut events = live.subscribe(0);
    go.add_permits(1);

    let streamed = timeout(Duration::from_secs(30), async {
        let mut streamed = None;
        let mut backlog = std::mem::take(&mut events.backlog).into_iter();
        loop {
            let event = match backlog.next() {
                Some(event) => event,
                None => events.live.recv().await.expect("the run streams"),
            };
            match event.event {
                RunEvent::Sources { sources } => streamed = Some(sources),
                RunEvent::RunFinished { .. } => return streamed,
                _ => {}
            }
        }
    })
    .await
    .expect("the run finishes");
    server.await.expect("the search was answered");

    let expected = vec![
        WebSource {
            url: "https://www.tokyocheapo.com/events/halloween/".into(),
            title: "Halloween in Tokyo".into(),
        },
        WebSource {
            url: "https://www.gaijinpot.com/halloween".into(),
            title: "Shibuya Halloween".into(),
        },
    ];
    assert_eq!(streamed.as_ref(), Some(&expected));

    let kept = user
        .session_sources(chat)
        .await
        .expect("sources read")
        .expect("the chat is alice's");
    assert_eq!(kept.len(), 1, "one turn used the web");
    assert_eq!(kept[0].sources, expected);
    let history = user
        .session_messages(chat)
        .await
        .expect("messages read")
        .expect("the chat is alice's");
    let asked = history.first().expect("the prompt is kept");
    assert_eq!(kept[0].message, asked.timestamp());
}

#[test]
fn project_chats_run_on_the_project_prompt() {
    use super::{ASSISTANT_PROMPT, research::LEAD_PROMPT, system_prompt};
    use crate::projects::turn::PROJECT_PROMPT;
    assert_eq!(system_prompt(false, true), PROJECT_PROMPT);
    assert_eq!(system_prompt(true, false), LEAD_PROMPT);
    assert_eq!(system_prompt(false, false), ASSISTANT_PROMPT);
}
