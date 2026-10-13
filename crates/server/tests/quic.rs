use async_trait::async_trait;
use beep_server::{
    app::{App, ServerTools, UserNotice},
    client::{Client, Node, OnNotice, Replies},
    cron,
    models::ModelChoices,
    protocol::{
        ApiError, AppKind, CancelRun, Chat, ConnectApp, CreateCron, DeletePlace, DeleteSkill,
        DeviceKind, Event, ExistingSessions, GetEvents, GetRun, GetSession, GetUpdates, Health,
        HistoryEntry, LinkDevice, ListApps, ListCrons, ListDevices, ListPlaces, ListSessions,
        ListSkills, MemorySource, MessageBody, Notice, RemoveDevice, RunEvent, Session,
        StartAppAuth, StreamUpdates, SuggestPrompts, UndoSkill, Update, UpdateLocation,
        UpdatePlace, UpdateSkill,
        link::{LinkOffer, OFFER_TTL},
        wire::framed,
    },
    push::Pusher,
    starters,
    store::{CreateRun, Fact, NewPlace, NewSkill, SignIn, TestDb},
    transport,
    user::User,
    wake::{self, Event as WakeEvent},
};
use chrono::Utc;
use flashmind::cron::{CronJob, JobSchedule};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role,
    StreamEvent,
};
use iroh::SecretKey;
use iroh::address_lookup::MemoryLookup;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use serde_json::json;
use std::{
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{sync::mpsc, time::timeout};
use uuid::Uuid;

/// Alice's key; the server knows her by its public half.
fn alice() -> SecretKey {
    SecretKey::from_bytes(&[1; 32])
}

struct ProviderStub {
    calls: AtomicUsize,
    /// The tools the last chat turn offered the model.
    tools: Mutex<Vec<String>>,
    /// The user messages the last chat turn sent the model.
    asked: Mutex<Vec<String>>,
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
        *self.tools.lock().unwrap() = request.tools.iter().map(|t| t.name.clone()).collect();
        *self.asked.lock().unwrap() = request
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .map(|m| m.content.clone())
            .collect();
        let is_tool_result = request
            .messages
            .last()
            .is_some_and(|m| m.role == Role::Tool);
        // An event the stub finds worth telling the user about.
        let worth_it = request
            .messages
            .iter()
            .any(|m| m.role == Role::User && m.content.starts_with("Worth it."));
        let slow = request
            .messages
            .iter()
            .any(|m| m.role == Role::User && m.content == "slow");
        Box::pin(async_stream::stream! {
            if slow {
                yield Ok(StreamEvent::ContentDelta("Working".into()));
                tokio::time::sleep(Duration::from_secs(60)).await;
            } else if !is_tool_result && worth_it {
                yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name: "notify_user".into() });
                yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: "{}".into() });
                yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
                return;
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
    db: TestDb,
    router: Router,
    app: Arc<App>,
    provider: Arc<ProviderStub>,
    /// The account alice's key signs into.
    alice_id: String,
}

impl Harness {
    async fn new() -> Self {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let provider = Arc::new(ProviderStub {
            calls: AtomicUsize::new(0),
            tools: Mutex::default(),
            asked: Mutex::default(),
        });
        let app = App::new(
            store,
            provider.clone(),
            AgentLlmConfig::new("openai:test".parse().unwrap()),
            ServerTools::default(),
            ModelChoices::only("openai:test".parse().unwrap()),
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
        pusher.start(app.notifications.subscribe());
        let router = Router::builder(endpoint)
            .accept(
                transport::ALPN,
                transport::BeepHandler::new(app.clone(), pusher),
            )
            .spawn();
        let alice_id = app
            .sign_in(&alice().public())
            .await
            .unwrap()
            .expect("a new key is never revoked")
            .user
            .id()
            .to_owned();
        Self {
            db,
            router,
            app,
            provider,
            alice_id,
        }
    }

    async fn client(&self) -> Phone {
        self.client_as(alice()).await
    }

    async fn client_as(&self, identity: SecretKey) -> Phone {
        let node = self.node(identity, Arc::new(|_| true)).await;
        let client = node.connect().await.unwrap();
        Phone {
            _node: node,
            client,
        }
    }

    async fn node(&self, identity: SecretKey, on_notice: OnNotice) -> Node {
        Node::bind(identity, self.router.endpoint().addr(), on_notice)
            .await
            .unwrap()
    }

    /// Sends `notice` to Alice the way a finished run or cron does.
    fn notify(&self, title: &str) {
        let notice = UserNotice {
            user: self.alice_id.clone(),
            notice: Notice {
                title: title.into(),
                body: String::new(),
                session_id: String::new(),
                app: None,
                approval: None,
                changed: false,
                group: false,
            },
        };
        self.app.notifications.send(notice).unwrap();
    }

    /// Lets the server dial `node` without a relay or pkarr.
    fn introduce(&self, node: &Node) {
        let lookup = MemoryLookup::new();
        lookup.add_endpoint_info(node.endpoint().addr());
        self.router.endpoint().address_lookup().unwrap().add(lookup);
    }
}

/// A phone's node and its connection, dropped together.
struct Phone {
    _node: Node,
    client: Client,
}

impl Deref for Phone {
    type Target = Client;
    fn deref(&self) -> &Client {
        &self.client
    }
}

/// Collects the titles of the notices a node takes. While `refuse` is set
/// it takes none, like a phone that cannot show them yet.
fn inbox() -> (OnNotice, mpsc::UnboundedReceiver<String>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let refuse = Arc::new(AtomicBool::new(false));
    let refusing = refuse.clone();
    let on_notice: OnNotice = Arc::new(move |notice: Notice| {
        if refusing.load(Ordering::SeqCst) {
            return false;
        }
        tx.send(notice.title).is_ok()
    });
    (on_notice, rx, refuse)
}

async fn next_title(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
    timeout(Duration::from_secs(20), rx.recv())
        .await
        .expect("a notice arrives")
        .expect("the inbox is open")
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

    let session = Uuid::new_v4();
    let chat = Chat {
        session_id: session,
        request_id: Uuid::new_v4(),
        message: "Remember I prefer tea".into(),
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
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
        [RunEvent::RunFinished { status, .. }] if status == "completed"
    ));
    let mut resumed = c.stream(&GetEvents { run_id, after: 3 }).await.unwrap();
    assert!(matches!(
        drain(&mut resumed).await.as_slice(),
        [RunEvent::RunFinished { status, .. }] if status == "completed"
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

    // Another key is another user, made on its first connection.
    let stranger = h.client_as(SecretKey::generate()).await;
    assert!(stranger.call(&ListSessions {}).await.unwrap().is_empty());
    let sessions = c.call(&ListSessions {}).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].updated_at > 0);

    // A phone asks which of its cached chats still exist.
    let ids = vec![session.to_string(), Uuid::new_v4().to_string()];
    let existing = ExistingSessions { ids };
    assert_eq!(c.call(&existing).await.unwrap(), [session.to_string()]);
    assert!(stranger.call(&existing).await.unwrap().is_empty());
    let alice_id = h.alice_id.clone();
    assert_ne!(alice_id, alice().public().to_string());
    assert!(h.app.user(&alice_id).is_some());

    let reopened = h.db.store().await.unwrap();
    assert_eq!(
        User::for_test(reopened.clone(), &alice_id)
            .memory_rows()
            .await
            .unwrap()[0]
            .content,
        "Prefers tea"
    );
    assert!(
        User::for_test(reopened.clone(), &alice_id)
            .session_messages(session)
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
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
    };
    let mut slow_events = c.stream(&slow).await.unwrap();
    let slow_run = started(&mut slow_events).await;

    let busy = Chat {
        session_id,
        request_id: Uuid::new_v4(),
        message: "again".into(),
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
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
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
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
            .any(|e| matches!(e, RunEvent::RunFinished { status, .. } if status == "cancelled"))
    );

    h.app.shutdown().await;
}

#[tokio::test]
async fn crash_recovery_and_memory_updates_are_durable_and_scoped() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let session = Uuid::new_v4();
    let run = match User::for_test(store.clone(), "alice")
        .create_run(session, "request", "hello")
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
        origin: None,
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
    let store = db.store().await.unwrap();
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
            .create_run(session, "new-request", "hello")
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
    assert_eq!(apps.len(), 14);
    assert_eq!(apps[0].slug, "gmail");
    assert_eq!(apps[13].kind, AppKind::Fastmail {});
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

#[tokio::test]
async fn a_key_signs_in_as_the_same_user_every_time() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let key = alice().public();
    let user = |signed: SignIn| match signed {
        SignIn::Account {
            account,
            device,
            primary,
        } => {
            assert_eq!(device, DeviceKind::Phone {});
            assert!(primary);
            account.id
        }
        SignIn::Revoked => panic!("a new key is never revoked"),
    };
    let first = user(store.sign_in(&key).await.unwrap());
    assert_ne!(first, key.to_string(), "an account id is its own");
    assert_eq!(user(store.sign_in(&key).await.unwrap()), first);
    let other = user(
        store
            .sign_in(&SecretKey::generate().public())
            .await
            .unwrap(),
    );
    assert_ne!(other, first);
    let users: Vec<String> = store
        .accounts()
        .await
        .unwrap()
        .into_iter()
        .map(|account| account.id)
        .collect();
    assert!(users.contains(&first) && users.contains(&other));
}

#[tokio::test]
async fn notices_reach_the_phone_open_idle_retried_or_after_being_away() {
    let h = Harness::new().await;
    let (on_notice, mut titles, refuse) = inbox();
    let node = h
        .node(alice(), on_notice)
        .await
        .with_idle_timeout(Duration::from_millis(500));
    h.introduce(&node);

    // Over the connection the phone opened.
    let c = node.connect().await.unwrap();
    c.call(&Health {}).await.unwrap();
    h.notify("open");
    assert_eq!(next_title(&mut titles).await, "open");

    // The connection closes once idle, and the server dials the phone.
    timeout(Duration::from_secs(5), async {
        while !c.is_closed() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("an idle connection closes");
    h.notify("dialed");
    assert_eq!(next_title(&mut titles).await, "dialed");

    // A notice the phone can't take waits for its next connection.
    refuse.store(true, Ordering::SeqCst);
    h.notify("queued");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(titles.try_recv().is_err());
    refuse.store(false, Ordering::SeqCst);
    let c = node.connect().await.unwrap();
    assert_eq!(next_title(&mut titles).await, "queued");
    drop(c);

    // The server keeps trying without waiting for the phone to connect.
    refuse.store(true, Ordering::SeqCst);
    h.notify("retried");
    tokio::time::sleep(Duration::from_secs(1)).await;
    refuse.store(false, Ordering::SeqCst);
    assert_eq!(next_title(&mut titles).await, "retried");
}

#[tokio::test]
async fn a_woken_agent_reaches_the_phone_only_when_it_asks_to() {
    let h = Harness::new().await;
    let (on_notice, mut titles, _) = inbox();
    let node = h.node(alice(), on_notice).await;
    h.introduce(&node);
    let c = node.connect().await.unwrap();
    c.call(&Health {}).await.unwrap();
    let user = h.app.user(&h.alice_id).unwrap();
    let event = |description: &str| WakeEvent {
        label: "Arrived in Singapore".into(),
        description: description.into(),
        data: json!({"city": "Singapore"}),
    };
    let finished = |run_id: String| {
        let app = h.app.clone();
        async move {
            timeout(Duration::from_secs(20), async {
                while app.live(&run_id).await.is_some() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .expect("the run finishes");
        }
    };
    // Change notices carry no title.
    let next_notice = async |titles: &mut mpsc::UnboundedReceiver<String>| loop {
        let title = next_title(titles).await;
        if !title.is_empty() {
            return title;
        }
    };

    // Without notify_user the chat goes away and nothing reaches the phone.
    let quiet = wake::wake(&h.app, &user, event("Not worth it."))
        .await
        .unwrap()
        .expect("a run starts");
    finished(quiet.id).await;
    let sessions = user.sessions().await.unwrap();
    assert!(sessions.iter().all(|s| s.id != quiet.session_id));

    // With it the reply is the notice, in a chat named after the event.
    let told = wake::wake(&h.app, &user, event("Worth it."))
        .await
        .unwrap()
        .expect("a run starts");
    finished(told.id).await;
    assert_eq!(next_notice(&mut titles).await, "Arrived in Singapore");
    let sessions = user.sessions().await.unwrap();
    let session = sessions.iter().find(|s| s.id == told.session_id).unwrap();
    assert_eq!(session.title, "Arrived in Singapore");
}

#[tokio::test]
async fn a_routine_runs_without_its_chats_history_and_replies_in_it() {
    let h = Harness::new().await;
    let c = h.client().await;
    let session = Uuid::new_v4();
    let mut replies = c
        .stream(&Chat {
            session_id: session,
            request_id: Uuid::new_v4(),
            message: "Remember I prefer tea".into(),
            images: Vec::new(),
            model: None,
            thinking: None,
            platform: None,
        })
        .await
        .unwrap();
    drain(&mut replies).await;

    let user = h.app.user(&h.alice_id).unwrap();
    let job = CronJob {
        id: Uuid::new_v4(),
        schedule: JobSchedule::Cron("0 9 * * *".into()),
        task: "Worth it. Remind the user to drink tea.".into(),
        enabled: true,
        once: false,
        metadata: json!({"user_id": h.alice_id, "label": "Tea", "session_id": session.to_string()}),
        created_at: Utc::now(),
        last_run: None,
    };
    let run = cron::run_job(&h.app, &user, &job)
        .await
        .unwrap()
        .expect("a run starts");
    assert_eq!(run.session_id, session.to_string());
    timeout(Duration::from_secs(20), async {
        while h.app.live(&run.id).await.is_some() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the run finishes");

    // The model saw the routine's note alone, not the chat before it.
    assert_eq!(
        *h.provider.asked.lock().unwrap(),
        ["Worth it. Remind the user to drink tea."]
    );
    // The chat keeps both turns, so a reply there knows what the routine said.
    let context = user.context(session).await.unwrap().expect("a context");
    let asked: Vec<&str> = context
        .conversation
        .entries()
        .iter()
        .filter(|entry| entry.is_user())
        .map(|entry| entry.content())
        .collect();
    assert_eq!(
        asked,
        [
            "Remember I prefer tea",
            "Worth it. Remind the user to drink tea."
        ]
    );
}

#[tokio::test]
async fn arriving_at_a_place_starts_its_routine_once() {
    let h = Harness::new().await;
    let c = h.client().await;
    let user = h.app.user(&h.alice_id).unwrap();
    let fix = |metres_north: f64, accuracy: f64| UpdateLocation {
        latitude: 41.4 + metres_north / 111_195.0,
        longitude: 2.2,
        accuracy,
        timezone: "Europe/Madrid".into(),
        place: None,
    };
    c.call(&fix(2000.0, 10.0)).await.unwrap();
    let gym = user
        .save_place(&NewPlace {
            name: "Gym",
            latitude: 41.4,
            longitude: 2.2,
            radius: 100,
            address: None,
            inside: false,
        })
        .await
        .unwrap();
    let id = gym.id.to_string();
    c.call(&CreateCron {
        label: "Log my weights".into(),
        prompt: "Remind me to log weights".into(),
        schedule: format!("arrive:{id}:daily"),
    })
    .await
    .unwrap();
    let places = c.call(&ListPlaces {}).await.unwrap();
    assert_eq!(places.len(), 1);
    assert_eq!((places[0].routines, places[0].here), (1, false));
    let last_run = async || {
        let crons = c.call(&ListCrons {}).await.unwrap();
        crons[0].last_run_at.clone()
    };

    // A blurry fix inside does not count; a sharp one starts the routine.
    c.call(&fix(20.0, 400.0)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(last_run().await, None);
    c.call(&fix(20.0, 15.0)).await.unwrap();
    let ran = timeout(Duration::from_secs(20), async {
        loop {
            if let Some(at) = last_run().await {
                return at;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the routine starts");
    assert!(c.call(&ListPlaces {}).await.unwrap()[0].here);

    // Jitter at the edge, then leaving and coming back the same day, run nothing more.
    for metres in [110.0, 90.0, 400.0, 10.0] {
        c.call(&fix(metres, 15.0)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(last_run().await, Some(ran));

    let renamed = c
        .call(&UpdatePlace {
            id: id.clone(),
            name: Some("Climbing gym".into()),
            radius: Some(250),
        })
        .await
        .unwrap();
    assert_eq!(
        (renamed.name.as_str(), renamed.radius),
        ("Climbing gym", 250)
    );
    let too_wide = c
        .call(&UpdatePlace {
            id: id.clone(),
            name: None,
            radius: Some(5000),
        })
        .await;
    assert!(too_wide.is_err());
    c.call(&DeletePlace { id }).await.unwrap();
    assert!(c.call(&ListPlaces {}).await.unwrap().is_empty());
    assert!(c.call(&ListCrons {}).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_user_edits_pauses_undoes_and_deletes_a_skill() {
    let h = Harness::new().await;
    let c = h.client().await;
    let user = h.app.user(&h.alice_id).unwrap();
    let skill = NewSkill {
        name: "Book padel",
        description: "Booking a padel court",
        body: "1. Ask for 19:00.",
    };
    h.app
        .store
        .create_skill(&user.skill_owner(), skill)
        .await
        .unwrap()
        .unwrap();
    let listed = c.call(&ListSkills {}).await.unwrap();
    assert_eq!(listed.len(), 1);
    let id = listed[0].id.clone();
    assert!(listed[0].enabled && !listed[0].edited && !listed[0].can_undo);
    assert_eq!(listed[0].group_id, None);

    let edit = |body: Option<&str>, enabled: Option<bool>| UpdateSkill {
        id: id.clone(),
        name: None,
        description: None,
        body: body.map(Into::into),
        enabled,
    };
    let edited = c
        .call(&edit(Some("1. Ask for 20:00."), None))
        .await
        .unwrap();
    assert!(edited.edited && edited.can_undo);
    let paused = c.call(&edit(None, Some(false))).await.unwrap();
    assert!(!paused.enabled);
    assert!(c.call(&edit(Some("  "), None)).await.is_err());

    let undone = c.call(&UndoSkill { id: id.clone() }).await.unwrap();
    assert_eq!(undone.body, "1. Ask for 19:00.");
    assert!(!undone.enabled && !undone.can_undo);

    c.call(&DeleteSkill { id: id.clone() }).await.unwrap();
    assert!(c.call(&ListSkills {}).await.unwrap().is_empty());
    assert_eq!(
        api_error(c.call(&DeleteSkill { id }).await.unwrap_err()),
        ApiError::NotFound {}
    );
}

/// The computer Alice links to her account.
fn macbook() -> SecretKey {
    SecretKey::from_bytes(&[7; 32])
}

#[tokio::test]
async fn a_linked_computer_shares_the_account_until_removed() {
    let h = Harness::new().await;
    let server = h.router.endpoint().id();
    let (phone_notice, mut phone_titles, _) = inbox();
    let phone_node = h.node(alice(), phone_notice).await;
    let phone = phone_node.connect().await.unwrap();

    let now = SystemTime::now();
    let refused = |offer: LinkOffer| {
        let phone = &phone;
        async move { api_error(phone.call(&LinkDevice { offer }).await.unwrap_err()) }
    };
    let elsewhere = SecretKey::generate().public();
    let wrong_server = LinkOffer::sign(&macbook(), elsewhere, "MacBook", now).unwrap();
    assert!(matches!(
        refused(wrong_server).await,
        ApiError::BadRequest { .. }
    ));
    let old = now - OFFER_TTL - Duration::from_secs(120);
    let expired = LinkOffer::sign(&macbook(), server, "MacBook", old).unwrap();
    assert!(matches!(
        refused(expired).await,
        ApiError::BadRequest { .. }
    ));
    let own_key = LinkOffer::sign(&alice(), server, "Phone", now).unwrap();
    assert!(matches!(refused(own_key).await, ApiError::Conflict { .. }));

    let offer = LinkOffer::sign(&macbook(), server, "MacBook", now).unwrap();
    let linked = phone
        .call(&LinkDevice {
            offer: offer.clone(),
        })
        .await
        .unwrap();
    assert_eq!(linked.kind, DeviceKind::Desktop {});
    assert_eq!(linked.id, macbook().public().to_string());
    assert!(!linked.primary && !linked.this_device);
    assert!(matches!(refused(offer).await, ApiError::Conflict { .. }));

    // The computer signs into Alice's account and takes her notices too.
    let (desk_notice, mut desk_titles, _) = inbox();
    let desk_node = h.node(macbook(), desk_notice).await;
    h.introduce(&desk_node);
    let desk = desk_node.connect().await.unwrap();
    let devices = desk.call(&ListDevices {}).await.unwrap();
    assert_eq!(devices.len(), 2);
    assert!(devices[0].primary && devices[0].id == alice().public().to_string());
    assert!(devices[1].this_device && !devices[1].primary);
    h.notify("Both");
    let mut phone_seen = next_title(&mut phone_titles).await;
    while phone_seen != "Both" {
        phone_seen = next_title(&mut phone_titles).await;
    }
    let mut desk_seen = next_title(&mut desk_titles).await;
    while desk_seen != "Both" {
        desk_seen = next_title(&mut desk_titles).await;
    }

    // Only the account's own key links others.
    let other = LinkOffer::sign(&SecretKey::generate(), server, "Laptop", now).unwrap();
    let error = desk.call(&LinkDevice { offer: other }).await.unwrap_err();
    assert!(matches!(api_error(error), ApiError::Unauthorized {}));

    // A chat from the computer gets no phone tools.
    let chat = Chat {
        session_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        message: "hello from the desk".into(),
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
    };
    let mut replies = desk.stream(&chat).await.unwrap();
    drain(&mut replies).await;
    let tools = h.provider.tools.lock().unwrap().clone();
    assert!(tools.iter().any(|name| name == "memory_store"));
    assert!(!tools.iter().any(|name| name == "set_alarm"));
    let sessions = phone.call(&ListSessions {}).await.unwrap();
    assert!(sessions.iter().any(|s| s.id == chat.session_id.to_string()));

    phone.call(&RemoveDevice { id: linked.id }).await.unwrap();
    assert_eq!(phone.call(&ListDevices {}).await.unwrap().len(), 1);
    let again = desk_node.connect().await.unwrap();
    assert!(again.call(&Health {}).await.is_err());
}

#[tokio::test]
async fn a_phone_from_before_hello_is_still_served() {
    let h = Harness::new().await;
    let endpoint = iroh::Endpoint::builder(presets::N0)
        .secret_key(alice())
        .bind()
        .await
        .unwrap();
    let connection = endpoint
        .connect(h.router.endpoint().addr(), transport::ALPN)
        .await
        .unwrap();
    // A released app opens straight with a request: the server must take it
    // as one, not as a malformed Hello.
    for _ in 0..2 {
        let (send, recv) = connection.open_bi().await.unwrap();
        let (mut tx, mut rx) = framed(send, recv);
        tx.request(&ListSessions {}).await.unwrap();
        tx.finish().unwrap();
        let sessions = timeout(Duration::from_secs(10), rx.reply::<Vec<Session>>())
            .await
            .expect("the server answers")
            .unwrap()
            .expect("a reply frame");
        assert!(sessions.is_empty());
    }
}

#[tokio::test]
async fn a_client_follows_its_log_and_fetches_only_what_is_new() {
    let h = Harness::new().await;
    let c = h.client().await;
    let mut newest = c.stream(&StreamUpdates {}).await.unwrap();
    let start = newest.next().await.unwrap().unwrap();
    let empty = c.call(&GetUpdates { after: start }).await.unwrap();
    assert!(empty.updates.is_empty() && !empty.more);

    let session = Uuid::new_v4();
    let mut replies = c
        .stream(&Chat {
            session_id: session,
            request_id: Uuid::new_v4(),
            message: "Remember I prefer tea".into(),
            images: Vec::new(),
            model: None,
            thinking: None,
            platform: None,
        })
        .await
        .unwrap();
    drain(&mut replies).await;

    // Each time the stream says the log moved, the client asks for what it
    // lacks, until the reply is saved.
    let replied = |updates: &[Update]| {
        updates.iter().any(|update| {
            matches!(update, Update::Message { message, .. }
                if matches!(message.body, MessageBody::Assistant { .. }))
        })
    };
    let page = loop {
        let page = c.call(&GetUpdates { after: start }).await.unwrap();
        if replied(&page.updates) {
            break page;
        }
        timeout(Duration::from_secs(10), newest.next())
            .await
            .expect("the log moves")
            .unwrap()
            .expect("the stream stays open");
    };
    let last = page.last;
    let Some(Update::Chat { chat, .. }) = page.updates.first() else {
        panic!("no chat first: {page:?}");
    };
    assert_eq!(chat.id, session);
    let texts: Vec<&str> = page
        .updates
        .iter()
        .filter_map(|update| match update {
            Update::Message {
                message, chat_id, ..
            } if *chat_id == session => match &message.body {
                MessageBody::User { content, .. } | MessageBody::Assistant { content } => {
                    Some(content.as_str())
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        ["Remember I prefer tea", "I remember your preference."]
    );
    assert!(
        c.call(&GetUpdates { after: last })
            .await
            .unwrap()
            .updates
            .is_empty()
    );

    // Older apps still read the same chat the old way.
    let history = c
        .call(&GetSession {
            id: session.to_string(),
        })
        .await
        .unwrap();
    assert!(
        matches!(history.first(), Some(HistoryEntry::User { content, .. }) if content == "Remember I prefer tea")
    );
}
