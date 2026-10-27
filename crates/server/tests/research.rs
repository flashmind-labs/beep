//! A research from the first message to the result, over QUIC, with every
//! agent played by a scripted model.

use async_trait::async_trait;
use beep_server::{
    app::{App, ServerTools},
    client::{Client, Node, Replies},
    models::ModelChoices,
    protocol::{
        Chat, Event, GetResearch, GetResearchPdf, GetSession, HistoryEntry, Research,
        ResearchEntryBody, ResearchStatus, RunEvent, StopResearch,
    },
    push::Pusher,
    research::{ResearchConfig, RoleModel},
    store::TestDb,
    transport,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Message,
    Provider, ReasoningLevel, Role, StreamEvent,
};
use iroh::{SecretKey, endpoint::presets, protocol::Router};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// What one scripted turn does: call a tool, or answer.
enum Step {
    Call(&'static str, Value),
    Say(&'static str),
}

/// Plays the lead, the scouts, the critic and the writer by their prompts.
struct TeamStub;

impl TeamStub {
    fn step(request: &CompletionRequest) -> Step {
        let system = request.messages[0].content.as_str();
        let task = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map_or("", |m| m.content.as_str());
        // Tool results since the task, so each turn moves one step on.
        let done = request
            .messages
            .iter()
            .rev()
            .take_while(|m| m.role != Role::User)
            .filter(|m: &&Message| m.role == Role::Tool)
            .count();
        let at = |steps: Vec<Step>| steps.into_iter().nth(done).unwrap_or(Step::Say("Done."));
        if system.starts_with("You keep the long-term memory") {
            return Step::Say(r#"{"ops":[]}"#);
        }
        if system.starts_with("You coordinate a research team") {
            let brief = json!({"text": "Lead with yes.", "to": "writer"});
            if task.contains("Round 1 ") {
                let gap = json!({"questions": [{"question": "Q gap"}]});
                return at(vec![
                    Step::Call("add_scouts", gap),
                    Step::Call("post_message", brief),
                    Step::Say("Ready."),
                ]);
            }
            return at(vec![Step::Call("post_message", brief), Step::Say("Ready.")]);
        }
        if system.contains("leading a research") {
            if task.contains("The team finished the research") {
                return Step::Say("Mostly yes. The full paper is in the research window.");
            }
            if task.contains("The team finished follow-up round") {
                return Step::Say("It is cheap. The paper is updated.");
            }
            if task.contains("cost") {
                let more = json!({"questions": [{"question": "Q cost"}]});
                return at(vec![
                    Step::Call("research_more", more),
                    Step::Say("Looking into the cost."),
                ]);
            }
            if task.lines().any(|line| line.trim() == "Go") {
                let questions =
                    json!({"questions": [{"question": "Q one"}, {"question": "Q two"}]});
                return at(vec![
                    Step::Call("start_research", questions),
                    Step::Say("Under way."),
                ]);
            }
            return Step::Say("Two questions, one scout each. Start?");
        }
        if system.starts_with("You are a scout") {
            if task.contains("The critic challenged") {
                let answer = json!({"text": "The source says so.", "to": "critic"});
                return at(vec![
                    Step::Call("post_message", answer),
                    Step::Say("Checked."),
                ]);
            }
            let url = format!("https://example.com/{}", Uuid::new_v4());
            let source = json!({"url": url, "title": "A report", "quote": "It works."});
            return at(vec![
                Step::Call("add_source", source),
                Step::Call("add_note", json!({"text": "It works.", "sources": [1]})),
                Step::Say("Found it."),
            ]);
        }
        if system.starts_with("You are the critic") {
            if task.contains("Read the draft") {
                let fix = json!({"text": "Add a caveat.", "to": "writer"});
                return at(vec![Step::Call("post_message", fix), Step::Say("Close.")]);
            }
            let challenge = json!({"text": "Which page?", "to": "scout-1"});
            return at(vec![
                Step::Call("read_notes", json!({})),
                Step::Call("post_message", challenge),
                Step::Say("Mostly solid."),
            ]);
        }
        if system.starts_with("You are the writer") {
            let markdown = if task.contains("The critic asks") {
                "# Paper\n\nYes [1], with a caveat."
            } else {
                "# Paper\n\nYes [1]."
            };
            return at(vec![
                Step::Call("write_draft", json!({ "markdown": markdown })),
                Step::Say("Wrote it."),
            ]);
        }
        Step::Say("Title")
    }
}

#[async_trait]
impl LlmProvider for TeamStub {
    fn name(&self) -> &str {
        "test"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        let step = Self::step(&request);
        Box::pin(async_stream::stream! {
            match step {
                Step::Call(name, args) => {
                    yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name: name.into() });
                    yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: args.to_string() });
                    yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
                }
                Step::Say(text) => {
                    yield Ok(StreamEvent::ContentDelta(text.into()));
                    yield Ok(StreamEvent::Finished(FinishReason::Stop));
                }
            }
        })
    }
}

struct Harness {
    _db: TestDb,
    router: Router,
    _app: Arc<App>,
}

impl Harness {
    async fn new() -> Self {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let model = "openai:test".parse().unwrap();
        let role = RoleModel {
            model: "openai:test".parse().unwrap(),
            thinking: ReasoningLevel::Medium,
        };
        let research = ResearchConfig {
            lead: role.clone(),
            scout: role.clone(),
            critic: role.clone(),
            writer: role,
            scout_choices: Vec::new(),
        };
        let app = App::new(
            store,
            Arc::new(TeamStub),
            AgentLlmConfig::new("openai:test".parse().unwrap()),
            ServerTools::default(),
            ModelChoices::only(model).with_research(Some(research)),
        );
        let endpoint = iroh::Endpoint::builder(presets::N0)
            .secret_key(SecretKey::generate())
            .bind()
            .await
            .unwrap();
        let pusher = Pusher::with_retry_waits(
            endpoint.clone(),
            Arc::new(app.store.clone()),
            None,
            vec![Duration::from_millis(200)],
        );
        let router = Router::builder(endpoint)
            .accept(
                transport::ALPN,
                transport::BeepHandler::new(app.clone(), pusher),
            )
            .spawn();
        Self {
            _db: db,
            router,
            _app: app,
        }
    }

    async fn client(&self) -> (Node, Client) {
        let node = Node::bind(
            SecretKey::from_bytes(&[1; 32]),
            self.router.endpoint().addr(),
            Arc::new(|_| true),
        )
        .await
        .unwrap();
        let client = node.connect().await.unwrap();
        (node, client)
    }
}

fn chat(session_id: Uuid, message: &str, research: bool) -> Chat {
    Chat {
        session_id,
        request_id: Uuid::new_v4(),
        message: message.into(),
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
        files: Vec::new(),
        research,
    }
}

/// Sends a message and returns the reply's text once the run ends.
async fn say(client: &Client, request: &Chat) -> String {
    let mut replies: Replies<Event> = client.stream(request).await.unwrap();
    let mut text = String::new();
    while let Some(event) = replies.next().await.unwrap() {
        if let RunEvent::TextDelta { delta } = event.event {
            text.push_str(&delta);
        }
    }
    text
}

async fn research(client: &Client, session_id: Uuid) -> Research {
    client
        .call(&GetResearch {
            session_id,
            after: 0,
        })
        .await
        .unwrap()
        .expect("the chat is a research")
}

/// Waits for the research to leave the running statuses.
async fn settled(client: &Client, session_id: Uuid) -> Research {
    timeout(Duration::from_secs(30), async {
        loop {
            let research = research(client, session_id).await;
            if !matches!(
                research.status,
                ResearchStatus::Researching {}
                    | ResearchStatus::Reviewing {}
                    | ResearchStatus::Writing {}
            ) {
                return research;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the team finishes")
}

/// Waits for an answer starting with `start` in the chat, and returns the
/// chat.
async fn replied(client: &Client, session: Uuid, start: &str) -> Vec<HistoryEntry> {
    timeout(Duration::from_secs(20), async {
        loop {
            let entries = client
                .call(&GetSession {
                    id: session.to_string(),
                })
                .await
                .unwrap();
            let replied = entries.iter().any(|e| {
                matches!(e, HistoryEntry::Assistant { content, .. } if content.starts_with(start))
            });
            if replied {
                return entries;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the result reaches the chat")
}

#[tokio::test]
async fn a_research_plans_runs_its_team_and_reports_back() {
    let h = Harness::new().await;
    let (_node, c) = h.client().await;
    let session = Uuid::new_v4();

    let plan = say(&c, &chat(session, "Does it work?", true)).await;
    assert_eq!(plan, "Two questions, one scout each. Start?");
    let planned = research(&c, session).await;
    assert_eq!(planned.status, ResearchStatus::Planning {});
    assert_eq!(planned.topic, "Does it work?");
    assert_eq!(planned.agents.len(), 1, "only the lead before the plan");

    assert_eq!(say(&c, &chat(session, "Go", false)).await, "Under way.");
    let done = settled(&c, session).await;
    assert_eq!(done.status, ResearchStatus::Done {});
    let ids: Vec<&str> = done.agents.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        ["lead", "scout-1", "scout-2", "scout-3", "critic", "writer"],
        "the lead put a scout on a gap"
    );
    assert_eq!(done.agents[1].role, "Q one");
    assert_eq!(done.agents[3].role, "Q gap");
    assert_eq!(done.round, 1);
    assert_eq!(done.sources.len(), 3);
    assert_eq!(done.draft, "# Paper\n\nYes [1], with a caveat.");
    let said = |from: &str, to: &str| {
        done.entries.iter().any(|e| {
            e.agent == from
                && matches!(&e.body, ResearchEntryBody::Said { to: Some(t), .. } if t == to)
        })
    };
    assert!(said("critic", "scout-1"), "the critic challenged a scout");
    assert!(said("scout-1", "critic"), "the scout answered");
    assert!(said("critic", "writer"), "the critic reviewed the draft");
    assert!(said("lead", "writer"), "the lead briefed the writer");
    let notes = done
        .entries
        .iter()
        .filter(|e| matches!(e.body, ResearchEntryBody::Note { .. }))
        .count();
    assert_eq!(notes, 3);
    assert!(done.finished_at.is_some());

    // The lead tells the user in the chat once the paper is done.
    let reported = replied(&c, session, "Mostly yes").await;
    assert!(
        !reported
            .iter()
            .any(|e| matches!(e, HistoryEntry::User { content, .. } if content.contains("The team finished"))),
        "the result turn's message stays out of the chat"
    );

    // The paper comes as a PDF.
    let pdf = c
        .call(&GetResearchPdf {
            session_id: session,
        })
        .await
        .unwrap();
    assert!(pdf.data.starts_with(b"%PDF"));
    assert_eq!(pdf.name, "Paper.pdf");

    // A follow-up sends the team on another round on the same paper.
    let follow_up = say(&c, &chat(session, "And what does it cost?", false)).await;
    assert_eq!(follow_up, "Looking into the cost.");
    let revised = settled(&c, session).await;
    assert_eq!(revised.status, ResearchStatus::Done {});
    assert_eq!(revised.round, 2);
    let cost = revised.agents.iter().find(|a| a.role == "Q cost");
    let cost = cost.expect("a scout on the follow-up");
    assert_eq!((cost.id.as_str(), cost.round), ("scout-4", 2));
    assert_eq!(revised.sources.len(), 4);
    replied(&c, session, "It is cheap").await;

    // A research that is done has nothing to stop.
    c.call(&StopResearch {
        session_id: session,
    })
    .await
    .unwrap();
    assert_eq!(research(&c, session).await.status, ResearchStatus::Done {});
}

#[tokio::test]
async fn an_ordinary_chat_has_no_research() {
    let h = Harness::new().await;
    let (_node, c) = h.client().await;
    let session = Uuid::new_v4();
    say(&c, &chat(session, "Hello", false)).await;
    let none = c
        .call(&GetResearch {
            session_id: session,
            after: 0,
        })
        .await
        .unwrap();
    assert!(none.is_none());
    let pdf = c
        .call(&GetResearchPdf {
            session_id: session,
        })
        .await;
    assert!(pdf.is_err(), "an ordinary chat has no paper");
}
