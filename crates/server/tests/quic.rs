use async_trait::async_trait;
use beep_server::{
    app::{App, ServerTools, UserNotice},
    client::{Client, Node, OnNotice, Replies},
    cron,
    models::ModelChoices,
    protocol::{
        ActiveRun, ApiError, AppKind, CancelRun, CancelWork, Chat, ConnectApp, CreateCron,
        DeletePlace, DeleteSkill, DeviceKind, DismissUpdates, Event, ExistingSessions, Filler,
        GetAgents, GetEvents, GetRun, GetSession, GetSessionAgents, GetSessionFiles, GetUpdates,
        Health, HistoryEntry, InviteDevice, LinkDevice, ListApps, ListCrons, ListDevices,
        ListPendingApprovals, ListPlaces, ListQuestions, ListSessions, ListSkills, ListUpdates,
        ListWork, MAX_SAY_CHARS, MemorySource, MessageBody, Notice, PeekInvite, RemoveDevice,
        RunEvent, Say, Session, SetAgents, SpeakNotice, SpokenTurns, StartAppAuth, StreamUpdates,
        SuggestPrompts, UndoSkill, Update, UpdateLocation, UpdatePlace, UpdateSkill, Upload,
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
use flashmind::cron::{CronJob, CronRegistry, JobSchedule};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role,
    StreamEvent, SttRequest, TtsRequest,
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
        if system.starts_with("You turn a message from Beep") {
            return Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta("Your tea order is ready. Want me to pay?".into()));
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
        // A routine the stub finds done: it tells the user and ends it.
        let done = request
            .messages
            .iter()
            .any(|m| m.role == Role::User && m.content.starts_with("Done."));
        let slow = request
            .messages
            .iter()
            .any(|m| m.role == Role::User && m.content == "slow");
        Box::pin(async_stream::stream! {
            if slow {
                yield Ok(StreamEvent::ContentDelta("Working".into()));
                tokio::time::sleep(Duration::from_secs(60)).await;
            } else if !is_tool_result && done {
                yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name: "notify_user".into() });
                yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: "{}".into() });
                yield Ok(StreamEvent::ToolCallStart { index: 1, id: Uuid::new_v4().to_string(), name: "end_routine".into() });
                yield Ok(StreamEvent::ToolCallDelta { index: 1, arguments: "{}".into() });
                yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
                return;
            } else if !is_tool_result && worth_it {
                yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name: "notify_user".into() });
                yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: "{}".into() });
                yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
                return;
            } else if !is_tool_result {
                yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name: "memory".into() });
                yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: json!({"action":"store","content":"Prefers tea"}).to_string() });
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
    fn text_to_speech(&self, request: TtsRequest) -> CompletionStream {
        let input = request.input;
        Box::pin(async_stream::stream! {
            yield Ok(StreamEvent::AudioDelta { data: input.into_bytes(), format: "mp3".into() });
            yield Ok(StreamEvent::Finished(FinishReason::Stop));
        })
    }
    fn transcribe(&self, _request: SttRequest) -> CompletionStream {
        Box::pin(async_stream::stream! {
            yield Ok(StreamEvent::ContentDelta("Remember I prefer tea".into()));
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
            desktop: None,
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
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
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
    assert_eq!(tools[0].name, "memory");
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
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
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
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
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
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
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
        scope: None,
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
    assert_eq!(apps.len(), 10);
    assert_eq!(apps[0].slug, "gmail");
    assert_eq!(apps[9].kind, AppKind::Fastmail {});
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
            files: Vec::new(),
            research: false,
            voice: None,
            spoken: false,
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
    let run = cron::run_job(&h.app, &user, &job, cron::WhenBusy::Wait)
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

/// Waits until run `run_id` is over.
async fn run_over(app: &App, run_id: &str) {
    timeout(Duration::from_secs(20), async {
        while app.live(run_id).await.is_some() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the run finishes");
}

/// A chat with one turn of the user's in it.
async fn chat_with_a_turn(c: &Client) -> Uuid {
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
            files: Vec::new(),
            research: false,
            voice: None,
            spoken: false,
        })
        .await
        .unwrap();
    drain(&mut replies).await;
    session
}

/// A routine of Alice's checking every 15 minutes with `task`, saved, set
/// up from `session` when there is one.
async fn watch(h: &Harness, task: &str, session: Option<Uuid>) -> CronJob {
    let tools = CronRegistry::new(Arc::new(cron::BeepCronStore::for_user(
        h.app.store.clone(),
        &h.alice_id,
    )));
    let mut metadata = json!({"user_id": h.alice_id, "label": "Sam's reply"});
    if let Some(session) = session {
        metadata["session_id"] = json!(session.to_string());
    }
    tools
        .create_with_metadata(
            JobSchedule::Cron("*/15 * * * *".into()),
            task.into(),
            false,
            metadata,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn a_routine_that_finds_nothing_leaves_nothing_behind() {
    let h = Harness::new().await;
    let c = h.client().await;
    let session = chat_with_a_turn(&c).await;
    let user = h.app.user(&h.alice_id).unwrap();
    let history = async || {
        c.call(&GetSession {
            id: session.to_string(),
        })
        .await
        .unwrap()
    };
    let before = history().await;
    let listed = |sessions: Vec<Session>, id: &str| sessions.into_iter().find(|s| s.id == id);
    let listed_before = listed(user.sessions().await.unwrap(), &session.to_string()).unwrap();

    let job = watch(&h, "Check whether Sam replied.", Some(session)).await;
    let run = cron::run_job(&h.app, &user, &job, cron::WhenBusy::Wait)
        .await
        .unwrap()
        .expect("a run starts");
    run_over(&h.app, &run.id).await;

    // The run could end itself but not make or change routines.
    let tools = h.provider.tools.lock().unwrap().clone();
    assert!(tools.iter().any(|t| t == "end_routine"), "{tools:?}");
    assert!(!tools.iter().any(|t| t == "cron_create"), "{tools:?}");
    // The chat shows nothing new and stays where it was in the list.
    assert_eq!(history().await, before);
    let listed_after = listed(user.sessions().await.unwrap(), &session.to_string()).unwrap();
    assert_eq!(listed_after.updated_at, listed_before.updated_at);
    // A quiet check goes on running.
    let routines = user.list_crons().await.unwrap();
    assert!(routines.iter().any(|r| r.id == job.id.to_string()));

    // A routine set up outside a chat leaves no empty chat behind.
    let loose = watch(&h, "Check whether Sam replied.", None).await;
    let run = cron::run_job(&h.app, &user, &loose, cron::WhenBusy::Wait)
        .await
        .unwrap()
        .expect("a run starts");
    run_over(&h.app, &run.id).await;
    assert!(listed(user.sessions().await.unwrap(), &run.session_id).is_none());
}

#[tokio::test]
async fn a_routine_tells_the_user_in_its_chat_once_and_ends_itself() {
    let h = Harness::new().await;
    let c = h.client().await;
    let session = chat_with_a_turn(&c).await;
    let user = h.app.user(&h.alice_id).unwrap();

    let job = watch(&h, "Done. Tell the user once Sam replies.", Some(session)).await;
    let run = cron::run_job(&h.app, &user, &job, cron::WhenBusy::Wait)
        .await
        .unwrap()
        .expect("a run starts");
    assert_eq!(run.session_id, session.to_string());
    run_over(&h.app, &run.id).await;

    // The chat gets the reply alone, with no routine turn and no tools.
    let history = c
        .call(&GetSession {
            id: session.to_string(),
        })
        .await
        .unwrap();
    let [
        HistoryEntry::User { .. },
        HistoryEntry::Assistant { tools: turn, .. },
        HistoryEntry::Assistant {
            content: told,
            tools,
            ..
        },
    ] = history.as_slice()
    else {
        panic!("unexpected history: {history:?}");
    };
    assert_eq!(turn.len(), 1, "the user's turn keeps its tools");
    assert_eq!(told, "I remember your preference.");
    assert!(tools.is_empty());
    // It ended itself, so it never runs again.
    let routines = user.list_crons().await.unwrap();
    assert!(routines.iter().all(|r| r.id != job.id.to_string()));
}

#[tokio::test]
async fn a_routine_waits_for_its_busy_chat_instead_of_opening_another() {
    let h = Harness::new().await;
    let c = h.client().await;
    let session = Uuid::new_v4();
    let mut slow = c
        .stream(&Chat {
            session_id: session,
            request_id: Uuid::new_v4(),
            message: "slow".into(),
            images: Vec::new(),
            model: None,
            thinking: None,
            platform: None,
            files: Vec::new(),
            research: false,
            voice: None,
            spoken: false,
        })
        .await
        .unwrap();
    let slow_run = started(&mut slow).await;
    let user = h.app.user(&h.alice_id).unwrap();
    let job = watch(&h, "Worth it. Tell the user Sam replied.", Some(session)).await;

    // Run now from the app: nothing starts, and no other chat opens.
    let chats = user.sessions().await.unwrap().len();
    let none = cron::run_job(&h.app, &user, &job, cron::WhenBusy::GiveUp)
        .await
        .unwrap();
    assert!(none.is_none());
    assert_eq!(user.sessions().await.unwrap().len(), chats);

    // The scheduler waits for the chat and runs there once it is free.
    let scheduled = {
        let (app, user, job) = (h.app.clone(), user.clone(), job.clone());
        tokio::spawn(async move { cron::run_job(&app, &user, &job, cron::WhenBusy::Wait).await })
    };
    c.call(&CancelRun { id: slow_run }).await.unwrap();
    let run = timeout(Duration::from_secs(20), scheduled)
        .await
        .expect("the routine starts once the chat is free")
        .unwrap()
        .unwrap()
        .expect("a run starts");
    assert_eq!(run.session_id, session.to_string());
    run_over(&h.app, &run.id).await;
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

/// Collects every notice a node takes.
fn notices() -> (OnNotice, mpsc::UnboundedReceiver<Notice>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let on_notice: OnNotice = Arc::new(move |notice: Notice| tx.send(notice).is_ok());
    (on_notice, rx)
}

/// The next notice titled `title`, skipping the others.
async fn next_titled(rx: &mut mpsc::UnboundedReceiver<Notice>, title: &str) -> Notice {
    timeout(Duration::from_secs(20), async {
        loop {
            let notice = rx.recv().await.expect("the inbox is open");
            if notice.title == title {
                return notice;
            }
        }
    })
    .await
    .expect("the notice arrives")
}

#[tokio::test]
async fn a_computer_gets_the_notice_meant_for_computers() {
    let h = Harness::new().await;
    let server = h.router.endpoint().id();
    let (phone_notice, mut phone_notices) = notices();
    let phone_node = h.node(alice(), phone_notice).await;
    h.introduce(&phone_node);
    let phone = phone_node.connect().await.unwrap();
    let offer = LinkOffer::sign(&macbook(), server, "MacBook", SystemTime::now()).unwrap();
    phone.call(&LinkDevice { offer }).await.unwrap();
    let (desk_notice, mut desk_notices) = notices();
    let desk_node = h.node(macbook(), desk_notice).await;
    h.introduce(&desk_node);
    let _desk = desk_node.connect().await.unwrap();

    let title = "Claude Code on build-box finished";
    let plain = Notice {
        title: title.into(),
        body: "Fix the login test: done".into(),
        session_id: String::new(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    };
    let key = "agent::ab12:claude:a3f9c1:build-box";
    h.app
        .notifications
        .send(UserNotice {
            user: h.alice_id.clone(),
            notice: plain.clone(),
            desktop: Some(Notice {
                session_id: key.into(),
                ..plain.clone()
            }),
        })
        .unwrap();
    assert_eq!(next_titled(&mut phone_notices, title).await, plain);
    assert_eq!(
        next_titled(&mut desk_notices, title).await.session_id,
        key,
        "the computer can open the session"
    );
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

    // Any key links others, not only the account's own.
    let other = LinkOffer::sign(&SecretKey::generate(), server, "Laptop", now).unwrap();
    let laptop = desk.call(&LinkDevice { offer: other }).await.unwrap();
    assert_eq!(laptop.name, "Laptop");
    phone.call(&RemoveDevice { id: laptop.id }).await.unwrap();
    // And shows a code for a new device to join with.
    let invite = desk
        .call(&InviteDevice {
            name: "Desk".into(),
        })
        .await
        .unwrap();
    let joiner = h.client_as(SecretKey::generate()).await;
    let preview = joiner
        .call(&PeekInvite { code: invite.code })
        .await
        .unwrap();
    assert_eq!(preview.name, "Desk");

    // A chat from the computer gets no phone tools.
    let chat = Chat {
        session_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        message: "hello from the desk".into(),
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
    };
    let mut replies = desk.stream(&chat).await.unwrap();
    drain(&mut replies).await;
    let tools = h.provider.tools.lock().unwrap().clone();
    assert!(tools.iter().any(|name| name == "memory"));
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
            files: Vec::new(),
            research: false,
            voice: None,
            spoken: false,
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

#[tokio::test]
async fn a_prompt_from_the_computer_shows_live_on_the_phone() {
    let h = Harness::new().await;
    let server = h.router.endpoint().id();
    let (phone_notice, mut phone_notices, _) = inbox();
    let phone_node = h.node(alice(), phone_notice).await;
    let phone = phone_node.connect().await.unwrap();
    let offer = LinkOffer::sign(&macbook(), server, "MacBook", SystemTime::now()).unwrap();
    phone.call(&LinkDevice { offer }).await.unwrap();
    let (desk_notice, _, _) = inbox();
    let desk_node = h.node(macbook(), desk_notice).await;
    h.introduce(&desk_node);
    let desk = desk_node.connect().await.unwrap();

    let session_id = Uuid::new_v4();
    let ask = Chat {
        session_id,
        request_id: Uuid::new_v4(),
        message: "slow".into(),
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
    };
    let mut asked = desk.stream(&ask).await.unwrap();
    let run_id = started(&mut asked).await;

    // The phone hears the chat changed, and finds it listed with the prompt
    // while the reply is still being written. Linking the computer sends a
    // notice too, so it looks again on each one, as the apps do.
    loop {
        next_title(&mut phone_notices).await;
        let history = phone
            .call(&GetSession {
                id: session_id.to_string(),
            })
            .await
            .unwrap();
        if matches!(
            history.as_slice(),
            [HistoryEntry::User { content, .. }] if content == "slow"
        ) {
            break;
        }
        assert!(history.is_empty(), "only the prompt shows: {history:?}");
    }
    let sessions = phone.call(&ListSessions {}).await.unwrap();
    assert!(sessions.iter().any(|s| s.id == session_id.to_string()));

    // It follows the run from the start.
    let running = phone.call(&ActiveRun { session_id }).await.unwrap();
    assert_eq!(running.map(|run| run.id).as_deref(), Some(run_id.as_str()));
    let mut followed = phone
        .stream(&GetEvents {
            run_id: run_id.clone(),
            after: 0,
        })
        .await
        .unwrap();
    assert_eq!(started(&mut followed).await, run_id);
    let mut text = String::new();
    while text.is_empty() {
        match followed.next().await.unwrap().map(|e| e.event) {
            Some(RunEvent::TextDelta { delta }) => text = delta,
            Some(_) => {}
            None => panic!("the run ended before writing"),
        }
    }
    assert_eq!(text, "Working");

    desk.call(&CancelRun { id: run_id }).await.unwrap();
    assert!(
        drain(&mut followed)
            .await
            .iter()
            .any(|event| matches!(event, RunEvent::RunFinished { .. }))
    );
    assert_eq!(phone.call(&ActiveRun { session_id }).await.unwrap(), None);
}

#[tokio::test]
async fn a_spoken_message_runs_as_its_transcript_and_is_marked_spoken() {
    let h = Harness::new().await;
    let c = h.client().await;
    let session = Uuid::new_v4();
    let chat = Chat {
        session_id: session,
        request_id: Uuid::new_v4(),
        message: String::new(),
        images: Vec::new(),
        model: None,
        thinking: None,
        platform: None,
        files: Vec::new(),
        research: false,
        voice: Some(Upload {
            name: "reply.wav".into(),
            mime: "audio/wav".into(),
            data: vec![1, 2, 3, 4],
        }),
        spoken: false,
    };
    let mut replies = c.stream(&chat).await.unwrap();
    started(&mut replies).await;
    let events = drain(&mut replies).await;
    assert!(matches!(
        events.first(),
        Some(RunEvent::Transcript { text }) if text == "Remember I prefer tea"
    ));
    assert!(matches!(events.last(), Some(RunEvent::RunFinished { .. })));
    let asked = h.provider.asked.lock().unwrap().clone();
    assert!(asked.iter().any(|m| m.contains("Remember I prefer tea")));

    let history = c
        .call(&GetSession {
            id: session.to_string(),
        })
        .await
        .unwrap();
    let Some(HistoryEntry::User {
        content, timestamp, ..
    }) = history.first()
    else {
        panic!("the prompt comes first");
    };
    assert_eq!(content, "Remember I prefer tea");
    // The recording is not kept; the turn is marked spoken instead.
    let files = c
        .call(&GetSessionFiles {
            session_id: session.to_string(),
        })
        .await
        .unwrap();
    assert!(files.iter().all(|sent| sent.files.is_empty()));
    let spoken = c
        .call(&SpokenTurns {
            session_id: session,
        })
        .await
        .unwrap();
    assert_eq!(spoken, vec![*timestamp]);

    // Something that is not audio is refused before anything runs.
    let mut bad = chat.clone();
    bad.request_id = Uuid::new_v4();
    bad.voice = Some(Upload {
        name: "notes.txt".into(),
        mime: "text/plain".into(),
        data: vec![1],
    });
    let err = c.stream(&bad).await.unwrap().next().await.unwrap_err();
    assert!(matches!(api_error(err), ApiError::BadRequest { .. }));
}

#[tokio::test]
async fn live_mode_reads_lines_and_says_what_it_does() {
    let h = Harness::new().await;
    let c = h.client().await;
    let said = c
        .call(&Say {
            text: "The root disk is 91% full.".into(),
        })
        .await
        .unwrap();
    assert_eq!(said.text, "The root disk is 91% full.");
    assert_eq!(said.audio.data, said.text.as_bytes());
    let filler = c
        .call(&Filler {
            step: "Running df -h on build-box".into(),
            said: "¿Está lleno el disco de build-box?".into(),
            still: false,
        })
        .await
        .unwrap();
    assert_eq!(filler.text, "Voy a mirar build-box.");
    let long = c
        .call(&Say {
            text: "a".repeat(MAX_SAY_CHARS + 1),
        })
        .await
        .unwrap_err();
    assert!(matches!(api_error(long), ApiError::BadRequest { .. }));
}

#[tokio::test]
async fn a_notice_comes_back_as_a_voice_note() {
    let h = Harness::new().await;
    let c = h.client().await;
    let note = c
        .call(&SpeakNotice {
            title: "Coffee shop".into(),
            body: "**Order 12** is ready: https://shop.example/12".into(),
        })
        .await
        .unwrap();
    assert_eq!(note.text, "Your tea order is ready. Want me to pay?");
    assert_eq!(note.audio.mime, "audio/mpeg");
    assert_eq!(note.audio.data, note.text.as_bytes());
    let empty = c
        .call(&SpeakNotice {
            title: " ".into(),
            body: String::new(),
        })
        .await
        .unwrap_err();
    assert!(matches!(api_error(empty), ApiError::BadRequest { .. }));
}

#[tokio::test]
async fn a_chat_lists_its_background_work_and_refuses_ids_it_does_not_have() {
    let h = Harness::new().await;
    let c = h.client().await;
    let session_id = Uuid::new_v4();
    let listed = c.call(&ListWork { session_id }).await.unwrap();
    assert!(listed.is_empty());
    let questions = c.call(&ListQuestions { session_id }).await.unwrap();
    assert!(questions.is_empty());
    let waiting = c.call(&ListPendingApprovals {}).await.unwrap();
    assert!(waiting.is_empty());
    let updates = c.call(&ListUpdates {}).await.unwrap();
    assert!(updates.is_empty());
    c.call(&DismissUpdates { ids: vec![1, 2] }).await.unwrap();
    for id in ["w1", "not an id"] {
        let refused = c
            .call(&CancelWork {
                session_id,
                id: id.into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(api_error(refused), ApiError::NotFound {}));
    }
}

#[tokio::test]
async fn the_removed_agents_answer_as_off_and_empty() {
    let h = Harness::new().await;
    let c = h.client().await;
    let stranger = h.client_as(SecretKey::generate()).await;

    let off = c.call(&GetAgents {}).await.unwrap();
    assert!(!off.enabled && off.agents.is_empty());
    // Turning them on is accepted and ignored.
    let still_off = c.call(&SetAgents { enabled: true }).await.unwrap();
    assert!(!still_off.enabled && still_off.agents.is_empty());
    assert_eq!(c.call(&GetAgents {}).await.unwrap(), off);

    let session = chat_with_a_turn(&c).await;
    let ask = GetSessionAgents {
        session_id: session.to_string(),
    };
    assert!(c.call(&ask).await.unwrap().is_empty());
    let error = stranger.call(&ask).await.unwrap_err();
    assert!(matches!(api_error(error), ApiError::NotFound {}));
}
