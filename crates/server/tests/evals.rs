//! Prompt evals: real turns against the configured model, each checked for
//! the behavior the assistant prompt and the follow-up writer promise. They
//! cost money and the model is not deterministic, so they are ignored by
//! default. `just eval` runs them with the dev config, and `just eval wake`
//! runs only the woken-agent cases.
use beep_server::{
    account::user::User,
    agent::decision::DecisionTool,
    agent::models::ModelChoices,
    api::transport,
    app::{App, ServerTools},
    client::Node,
    config::Config,
    memory::memories,
    memory::starters::Generator,
    protocol::{Chat, GetEvents, MemorySource, RunEvent},
    push::Pusher,
    routines::wake,
    store::{Fact, Location, TestDb, Validity},
    tools::Transitous,
    tools::search::SearchTool,
};
use chrono::{Duration as Days, Local, NaiveDate, Utc};
use flashmind::{
    llm::OpenRouterProvider,
    types::{AgentLlmConfig, Model},
};
use futures::{StreamExt, future::join_all, stream};
use iroh::{SecretKey, endpoint::presets, protocol::Router};
use secrecy::SecretString;
use serde::Deserialize;
use std::{env, fs, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::timeout;
use uuid::Uuid;

/// The longest follow-up `suggest_replies` keeps; the writer is asked for 32.
const MAX_FOLLOW_UP_CHARS: usize = 40;
const TURN_TIMEOUT: Duration = Duration::from_secs(240);
/// Turns started together, under the server's limit of 16 running at once.
const MAX_TURNS_AT_ONCE: usize = 12;

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
    /// What each memory search returned, for reading a failed case.
    recalled: Vec<String>,
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
                RunEvent::ToolStarted {
                    name, description, ..
                } if name != "suggest_replies" => {
                    if name == "memory" {
                        turn.recalled.push(description);
                    }
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
                RunEvent::ToolResult { name, output, .. } => {
                    if name == "memory" {
                        turn.recalled.push(output);
                    }
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
    app: Arc<App>,
    router: Router,
}

impl Server {
    async fn start(config: &Config) -> Self {
        let db = TestDb::new()
            .await
            .expect("BEEP_TEST_DATABASE_URL names a database");
        // Memory search by meaning needs the embedder production uses.
        let embedder = memories::embedder(config).expect("the embedder builds");
        let store = db
            .store_with(embedder)
            .await
            .expect("the test schema opens");
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
            transitous: Some(Arc::new(
                Transitous::new().expect("the Transitous client builds"),
            )),
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
            .accept(
                transport::ALPN,
                transport::BeepHandler::new(app.clone(), pusher),
            )
            .spawn();
        Self {
            _db: db,
            app,
            router,
        }
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
            platform: None,
            files: Vec::new(),
            research: false,
            voice: None,
            spoken: false,
        };
        let mut replies = client.stream(&chat).await.expect("the chat starts");
        let mut events = Vec::new();
        // The server ends each stream after a window, as it does for the app,
        // so a long turn is followed with `GetEvents` until it finishes.
        let read = async {
            let mut run_id = None;
            let mut after = 0;
            loop {
                while let Some(event) = replies.next().await.expect("the stream reads") {
                    after = event.seq;
                    if let RunEvent::RunStarted { run_id: id, .. } = &event.event {
                        run_id = Some(id.clone());
                    }
                    events.push(event.event);
                }
                let finished = events
                    .iter()
                    .any(|e| matches!(e, RunEvent::RunFinished { .. }));
                let Some(run_id) = run_id.clone().filter(|_| !finished) else {
                    break;
                };
                replies = client
                    .stream(&GetEvents { run_id, after })
                    .await
                    .expect("the events resume");
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
    let cases: Vec<&Case> = CASES.iter().collect();
    let outcomes = run_cases(&config, &cases).await;
    let failed = outcomes.iter().filter(|o| o.ran && !o.passed).count();
    assert_eq!(failed, 0, "{failed} evals failed");
}

/// How one case went. Cases the `BEEP_EVAL` filter leaves out did not run.
#[derive(Default)]
struct Outcome {
    ran: bool,
    passed: bool,
}

/// Runs `cases` on a fresh server and prints each
/// verdict. Returns how each case went, in order.
async fn run_cases(config: &Config, cases: &[&Case]) -> Vec<Outcome> {
    let only = env::var("BEEP_EVAL").ok();
    let picked: Vec<bool> = cases
        .iter()
        .map(|case| only.as_deref().is_none_or(|only| case.name.contains(only)))
        .collect();
    let mut outcomes: Vec<Outcome> = cases.iter().map(|_| Outcome::default()).collect();
    if !picked.contains(&true) {
        return outcomes;
    }
    let server = Server::start(config).await;
    let turns: Vec<Turn> = stream::iter(
        cases
            .iter()
            .zip(&picked)
            .filter(|(_, picked)| **picked)
            .map(|(case, _)| server.turn(case.message)),
    )
    .buffered(MAX_TURNS_AT_ONCE)
    .collect()
    .await;

    let ran = cases
        .iter()
        .zip(outcomes.iter_mut())
        .zip(&picked)
        .filter(|(_, picked)| **picked)
        .map(|(pair, _)| pair);
    for ((case, outcome), turn) in ran.zip(&turns) {
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
        *outcome = Outcome {
            ran: true,
            passed: failures.is_empty(),
        };
    }
    outcomes
}

/// The judge for woken runs: the newest small model from another family
/// than the assistant's, at the same price.
const WAKE_JUDGE_MODEL: &str = "openrouter:anthropic/claude-haiku-5.5";

const WAKE_JUDGE: &str = "You grade an assistant that was woken by an event on the user's \
phone while nobody was chatting. It could send one notification or stay silent. You get \
what it knew about the user, the event, whether it notified, and its final reply (the \
notification text when it notified, its private reason otherwise).
A run passes only when all of these hold:
- notifying was the right call: the event plus what it knew gives the user something \
useful or timely now; a move with nothing known about it, or coming back home, is not \
worth an interruption;
- when it notified, the text is one or two short sentences of plain text without \
Markdown, specific to this user and event, and states nothing the context does not \
support;
- when it stayed silent, its reason is sound.
Answer with one line, \"PASS\" or \"FAIL: <short reason>\", and nothing else.";

/// A city the phone reports, as Android names it.
struct Fix {
    place: &'static str,
    latitude: f64,
    longitude: f64,
    timezone: &'static str,
}

const BARCELONA: Fix = Fix {
    place: "Barcelona, Catalonia, Spain",
    latitude: 41.39,
    longitude: 2.16,
    timezone: "Europe/Madrid",
};
const GIRONA: Fix = Fix {
    place: "Girona, Catalonia, Spain",
    latitude: 41.98,
    longitude: 2.82,
    timezone: "Europe/Madrid",
};
const TARRAGONA: Fix = Fix {
    place: "Tarragona, Catalonia, Spain",
    latitude: 41.12,
    longitude: 1.25,
    timezone: "Europe/Madrid",
};
const LISBON: Fix = Fix {
    place: "Lisbon, Lisbon, Portugal",
    latitude: 38.72,
    longitude: -9.14,
    timezone: "Europe/Lisbon",
};
const NEW_YORK: Fix = Fix {
    place: "New York, New York, United States",
    latitude: 40.71,
    longitude: -74.01,
    timezone: "America/New_York",
};

impl Fix {
    fn location(&self, hours_ago: i64) -> Location {
        Location {
            latitude: self.latitude,
            longitude: self.longitude,
            accuracy: 20.0,
            timezone: self.timezone.to_owned(),
            place: Some(self.place.to_owned()),
            updated_at: Utc::now() - Days::hours(hours_ago),
        }
    }
}

/// A memory the user has before the event, valid between two days counted
/// from today, either end open.
struct Memory {
    content: &'static str,
    from: Option<i64>,
    until: Option<i64>,
}

const fn always(content: &'static str) -> Memory {
    Memory {
        content,
        from: None,
        until: None,
    }
}

struct WakeCase {
    name: &'static str,
    memories: &'static [Memory],
    from: Fix,
    to: Fix,
    /// Whether it must notify; `None` leaves the call to the judge.
    notifies: Option<bool>,
}

const WAKE_CASES: &[WakeCase] = &[
    WakeCase {
        name: "wake: lands on a planned trip",
        memories: &[
            Memory {
                content: "Is in New York for a payments conference, staying at a hotel \
                          near the venue",
                from: Some(0),
                until: Some(4),
            },
            Memory {
                content: "Gives a talk at the conference on the second day at 10:00",
                from: Some(1),
                until: Some(1),
            },
            always("Lives in Barcelona"),
        ],
        from: BARCELONA,
        to: NEW_YORK,
        notifies: Some(true),
    },
    WakeCase {
        name: "wake: a new time zone with a daily medication",
        memories: &[
            always("Takes levothyroxine every morning at 7:00, before breakfast"),
            always("Lives in Barcelona"),
        ],
        from: BARCELONA,
        to: NEW_YORK,
        notifies: Some(true),
    },
    WakeCase {
        name: "wake: a day trip with nothing known",
        memories: &[always("Lives in Barcelona")],
        from: BARCELONA,
        to: TARRAGONA,
        notifies: Some(false),
    },
    WakeCase {
        name: "wake: back home",
        memories: &[
            always("Lives in Barcelona"),
            Memory {
                content: "Was in Lisbon for a client workshop",
                from: Some(-4),
                until: Some(-1),
            },
        ],
        from: LISBON,
        to: BARCELONA,
        notifies: Some(false),
    },
    WakeCase {
        name: "wake: a city where a friend lives",
        memories: &[
            always("Lives in Barcelona"),
            always("Their sister Laia lives in Girona and runs a bakery there"),
        ],
        from: BARCELONA,
        to: GIRONA,
        notifies: None,
    },
];

/// What a woken run did.
struct Woken {
    /// The event as the model got it.
    event: String,
    turn: Turn,
    notified: bool,
    /// Whether the chat is still in the user's list.
    kept: bool,
    title: Option<String>,
}

impl Server {
    /// Seeds a new user with the case's memories and location, wakes the
    /// agent on the arrival, and reads the run to its end.
    async fn wake(&self, case: &WakeCase) -> Woken {
        let user = self
            .app
            .sign_in(&SecretKey::generate().public())
            .await
            .expect("the store signs in")
            .expect("a new key is not revoked")
            .user;
        let today = Local::now().date_naive();
        for memory in case.memories {
            seed(&user, memory, today).await;
        }
        let (before, now) = (case.from.location(3), case.to.location(0));
        user.update_location(&now)
            .await
            .expect("the location saves");
        let event = wake::arrival(&before, &now).expect("the case is an arrival");
        let told = format!("{}\nDetails: {}", event.description, event.data);
        let run = wake::wake(&self.app, &user, event)
            .await
            .expect("the run submits")
            .expect("a run starts");

        let mut events = Vec::new();
        let read = async {
            let Some(live) = self.app.live(&run.id).await else {
                return;
            };
            let mut feed = live.subscribe(0);
            events.extend(feed.backlog.into_iter().map(|e| e.event));
            while !events
                .iter()
                .any(|e| matches!(e, RunEvent::RunFinished { .. }))
            {
                match feed.live.recv().await {
                    Ok(event) => events.push(event.event),
                    Err(_) => break,
                }
            }
            // The chat is kept or deleted after `RunFinished`.
            while self.app.live(&run.id).await.is_some() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        if timeout(TURN_TIMEOUT, read).await.is_err() {
            events.push(RunEvent::Error {
                message: "timed out".into(),
            });
        }
        let turn = Turn::read(events);
        let session = user
            .sessions()
            .await
            .expect("the sessions list")
            .into_iter()
            .find(|s| s.id == run.session_id);
        Woken {
            event: told,
            notified: turn.tools.iter().any(|t| t == "notify_user"),
            kept: session.is_some(),
            title: session.map(|s| s.title),
            turn,
        }
    }
}

async fn seed(user: &User, memory: &Memory, today: NaiveDate) {
    let day = |offset: Option<i64>| offset.map(|days| today + Days::days(days));
    let valid = (memory.from.is_some() || memory.until.is_some()).then(|| Validity {
        from: day(memory.from),
        until: day(memory.until),
    });
    user.add_memory(Fact {
        content: memory.content,
        category: None,
        source: MemorySource::User {},
        valid,
        origin: None,
        scope: None,
    })
    .await
    .expect("the memory saves");
}

/// What the judge reads about a case and its run.
fn wake_report(case: &WakeCase, woken: &Woken) -> String {
    let memories: String = case
        .memories
        .iter()
        .map(|m| {
            format!(
                "\n- {} (days {:?} to {:?} from today)",
                m.content, m.from, m.until
            )
        })
        .collect();
    format!(
        "What it knew:{memories}\n\nEvent:\n{}\n\nTools it \
         called: {:?}\nNotified: {}\n\nFinal reply:\n{}",
        woken.event,
        woken.turn.tools,
        woken.notified,
        woken.turn.reply.trim()
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the real model; run with `just eval wake`"]
async fn wake_evals() {
    let config = config();
    let only = env::var("BEEP_EVAL").ok();
    let cases: Vec<&WakeCase> = WAKE_CASES
        .iter()
        .filter(|case| only.as_deref().is_none_or(|only| case.name.contains(only)))
        .collect();
    if cases.is_empty() {
        return;
    }
    let server = Server::start(&config).await;
    let key = config
        .provider_key
        .as_deref()
        .expect("the config has a provider_key");
    let judge_model: Model = WAKE_JUDGE_MODEL.parse().expect("the judge model parses");
    let judge_llm = AgentLlmConfig::new(judge_model);
    let judge_provider = OpenRouterProvider::new(key.to_owned());
    let judge = Generator {
        provider: &judge_provider,
        llm: &judge_llm,
        user: "eval",
    };

    let runs = join_all(cases.iter().map(|case| server.wake(case))).await;
    let mut failed = 0;
    for (case, woken) in cases.iter().zip(&runs) {
        let mut failures: Vec<String> = woken
            .turn
            .errors
            .iter()
            .map(|e| format!("error: {e}"))
            .collect();
        if let Some(wanted) = case.notifies
            && wanted != woken.notified
        {
            failures.push(format!("notified={}, wanted {wanted}", woken.notified));
        }
        if woken.kept != woken.notified {
            failures.push(format!(
                "chat kept={} but notified={}",
                woken.kept, woken.notified
            ));
        }
        let report = wake_report(case, woken);
        let verdict = judge
            .text(WAKE_JUDGE, report)
            .await
            .expect("the judge answers");
        let verdict = verdict.trim();
        if !verdict.starts_with("PASS") {
            failures.push(format!("judge: {verdict}"));
        }

        let result = if failures.is_empty() { "PASS" } else { "FAIL" };
        println!("{result} {}", case.name);
        println!("  tools: {:?}", woken.turn.tools);
        println!("  notified: {} title: {:?}", woken.notified, woken.title);
        for recalled in &woken.turn.recalled {
            println!("  recalled: {}", recalled.replace('\n', " | "));
        }
        println!("  reply: {}", woken.turn.reply.trim().replace('\n', " "));
        for failure in &failures {
            println!("  - {failure}");
        }
        failed += usize::from(!failures.is_empty());
    }
    assert_eq!(failed, 0, "{failed} of {} wake evals failed", cases.len());
}
