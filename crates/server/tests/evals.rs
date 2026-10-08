//! Prompt evals: real turns against the configured model, each checked for
//! the behavior the assistant prompt and the follow-up writer promise. They
//! cost money and the model is not deterministic, so they are ignored by
//! default. `just eval` runs them with the dev config.
use beep_server::{
    app::{App, ServerTools},
    client::Node,
    config::Config,
    decision::DecisionTool,
    models::ModelChoices,
    protocol::{Chat, RunEvent},
    push::Pusher,
    search::SearchTool,
    store::TestDb,
    transport,
};
use flashmind::{
    llm::OpenRouterProvider,
    types::{AgentLlmConfig, Model},
};
use futures::future::join_all;
use iroh::{SecretKey, endpoint::presets, protocol::Router};
use secrecy::SecretString;
use serde::Deserialize;
use std::{env, fs, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::timeout;
use uuid::Uuid;

/// The longest follow-up the writer is asked for.
const MAX_FOLLOW_UP_CHARS: usize = 32;
const TURN_TIMEOUT: Duration = Duration::from_secs(240);

/// What a case expects of its turn.
enum Check {
    /// The tool is called at least once.
    Calls(&'static str),
    /// The tool is never called.
    Skips(&'static str),
    /// At least this many calls to the tool start before any of them returns.
    Parallel(&'static str, usize),
    /// Two or three follow-ups, each short enough.
    FollowUps,
    NoFollowUps,
}

struct Case {
    name: &'static str,
    message: &'static str,
    checks: &'static [Check],
}

const CASES: &[Case] = &[
    Case {
        name: "searches for the weather",
        message: "What's the weather in Lisbon tomorrow?",
        checks: &[Check::Calls("web_search")],
    },
    Case {
        name: "searches for recent news",
        message: "Who won the most recent Formula 1 race?",
        checks: &[Check::Calls("web_search")],
    },
    Case {
        name: "answers settled facts without searching",
        message: "What is the capital of Australia?",
        checks: &[Check::Skips("web_search")],
    },
    Case {
        name: "searches in parallel",
        message: "Compare the weather this weekend in Paris and in Rome.",
        checks: &[Check::Parallel("web_search", 2)],
    },
    Case {
        name: "explains with follow-ups",
        message: "Explain how vaccines train the immune system.",
        checks: &[Check::FollowUps],
    },
    Case {
        name: "no follow-ups after thanks",
        message: "Thanks, that's all for now!",
        checks: &[Check::NoFollowUps, Check::Skips("web_search")],
    },
    Case {
        name: "rides go to the map card",
        message: "Get me a taxi from Lisbon airport to Rossio square.",
        checks: &[Check::Calls("open_maps"), Check::Skips("open_app")],
    },
];

/// What a turn did, read from its event stream.
#[derive(Default)]
struct Turn {
    reply: String,
    /// Tool calls in the order they started, follow-ups left out.
    tools: Vec<String>,
    /// The most calls running at once.
    widest: Vec<String>,
    follow_ups: Option<Vec<String>>,
    errors: Vec<String>,
}

#[derive(Deserialize)]
struct Suggestions {
    replies: Vec<String>,
}

impl Turn {
    fn read(events: Vec<RunEvent>) -> Self {
        let mut turn = Turn::default();
        let mut running = Vec::new();
        for event in events {
            match event {
                RunEvent::TextDelta { delta } => turn.reply.push_str(&delta),
                RunEvent::ToolStarted { name, .. } if name != "suggest_replies" => {
                    turn.tools.push(name.clone());
                    running.push(name);
                    if running.len() > turn.widest.len() {
                        turn.widest = running.clone();
                    }
                }
                RunEvent::ToolResult { name, output, .. } if name == "suggest_replies" => {
                    turn.follow_ups = serde_json::from_str::<Suggestions>(&output)
                        .ok()
                        .map(|s| s.replies);
                }
                RunEvent::ToolResult { name, .. } => {
                    if let Some(at) = running.iter().position(|n| *n == name) {
                        running.remove(at);
                    }
                }
                RunEvent::Error { message } => turn.errors.push(message),
                _ => {}
            }
        }
        turn
    }

    /// Why `check` failed, or `None` when it passed.
    fn failure(&self, check: &Check) -> Option<String> {
        let called = |tool: &str| self.tools.iter().filter(|t| *t == tool).count();
        match *check {
            Check::Calls(tool) => (called(tool) == 0).then(|| format!("never called {tool}")),
            Check::Skips(tool) => (called(tool) > 0).then(|| format!("called {tool}")),
            Check::Parallel(tool, at_least) => {
                let wide = self.widest.iter().filter(|t| *t == tool).count();
                (wide < at_least)
                    .then(|| format!("at most {wide} {tool} calls at once, wanted {at_least}"))
            }
            Check::FollowUps => match &self.follow_ups {
                None => Some("no follow-ups".into()),
                Some(replies) if !(2..=3).contains(&replies.len()) => {
                    Some(format!("{} follow-ups", replies.len()))
                }
                Some(replies) => replies
                    .iter()
                    .find(|r| r.chars().count() > MAX_FOLLOW_UP_CHARS)
                    .map(|r| format!("follow-up over {MAX_FOLLOW_UP_CHARS} chars: {r}")),
            },
            Check::NoFollowUps => self
                .follow_ups
                .as_ref()
                .map(|replies| format!("follow-ups: {replies:?}")),
        }
    }
}

/// The server as `main` builds it, without the sandbox and Composio, on a
/// throwaway schema.
struct Server {
    _db: TestDb,
    router: Router,
}

impl Server {
    async fn start(config: &Config) -> Self {
        let db = TestDb::new()
            .await
            .expect("BEEP_TEST_DATABASE_URL names a database");
        let store = db.store().await.expect("the test schema opens");
        let key = config
            .provider_key
            .as_deref()
            .expect("the config has a provider_key");
        let model: Model = config.model.parse().expect("the config model parses");
        let mut llm = AgentLlmConfig::new(model).with_reasoning(config.reasoning);
        llm.max_tokens = Some(8192);
        llm.provider_preferences = Some(config.routing.clone());
        let provider = Arc::new(OpenRouterProvider::new(key.to_owned()));
        let models = ModelChoices::resolve(
            provider.as_ref(),
            llm.model.clone(),
            config.model_choices.clone(),
            &config.admins,
        )
        .await
        .with_routing(config.model_routing());
        let search = config
            .firecrawl_key
            .as_ref()
            .map(|key| Arc::new(SearchTool::new(key.clone()).expect("the search tool builds")));
        let decision = DecisionTool::new(SecretString::from(key)).expect("Jev builds");
        let tools = ServerTools {
            search,
            firecrawl_key: config.firecrawl_key.as_deref().map(SecretString::from),
            decision: Some(Arc::new(decision)),
            ..ServerTools::default()
        };
        let app = App::new(store, provider, llm, tools, models);
        let endpoint = iroh::Endpoint::builder(presets::N0)
            .secret_key(SecretKey::generate())
            .bind()
            .await
            .expect("the server endpoint binds");
        let pusher = Pusher::new(endpoint.clone(), Arc::new(app.store.clone()), None);
        pusher.start(app.notifications.subscribe());
        let router = Router::builder(endpoint)
            .accept(transport::ALPN, transport::BeepHandler::new(app, pusher))
            .spawn();
        Self { _db: db, router }
    }

    /// Sends `message` in a new chat and reads the turn to its end.
    async fn turn(&self, message: &str) -> Turn {
        let node = Node::bind(
            SecretKey::generate(),
            self.router.endpoint().addr(),
            Arc::new(|_| true),
        )
        .await
        .expect("the phone endpoint binds");
        let client = node.connect().await.expect("the phone connects");
        let chat = Chat {
            session_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            message: message.into(),
            images: Vec::new(),
            model: None,
            thinking: None,
        };
        let mut replies = client.stream(&chat).await.expect("the chat starts");
        let mut events = Vec::new();
        let read = async {
            while let Some(event) = replies.next().await.expect("the stream reads") {
                events.push(event.event);
            }
        };
        if timeout(TURN_TIMEOUT, read).await.is_err() {
            events.push(RunEvent::Error {
                message: "timed out".into(),
            });
        }
        Turn::read(events)
    }
}

fn config() -> Config {
    let path = env::var_os("BEEP_EVAL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.dev/android-server.toml")
        });
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    toml::from_str(&text).expect("the eval config parses")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the real model; run with `just eval`"]
async fn prompt_evals() {
    let config = config();
    let server = Server::start(&config).await;
    let only = env::var("BEEP_EVAL").ok();
    let cases: Vec<&Case> = CASES
        .iter()
        .filter(|case| only.as_deref().is_none_or(|only| case.name.contains(only)))
        .collect();
    let turns = join_all(cases.iter().map(|case| server.turn(case.message))).await;

    let mut failed = 0;
    for (case, turn) in cases.iter().zip(&turns) {
        let mut failures: Vec<String> =
            case.checks.iter().filter_map(|c| turn.failure(c)).collect();
        failures.extend(turn.errors.iter().map(|e| format!("error: {e}")));
        let verdict = if failures.is_empty() { "PASS" } else { "FAIL" };
        println!("{verdict} {}", case.name);
        println!("  tools: {:?}", turn.tools);
        println!("  follow-ups: {:?}", turn.follow_ups);
        let reply: String = turn.reply.chars().take(160).collect();
        println!("  reply: {}", reply.replace('\n', " "));
        for failure in &failures {
            println!("  - {failure}");
        }
        failed += usize::from(!failures.is_empty());
    }
    assert_eq!(failed, 0, "{failed} of {} evals failed", cases.len());
}
