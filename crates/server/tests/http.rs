//! The apps' HTTP end to end: a client on a real socket calls requests and
//! follows its events stream.

use async_trait::async_trait;
use beep_server::{
    agent::models::ModelChoices,
    api::http,
    app::{App, Reach, ServerTools, UserNotice},
    protocol::{
        AckNotice, AppEvent, Health, ListSessions, Notice,
        client::{Client, Events},
    },
    push::Pulse,
    store::TestDb,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::SecretKey;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;

/// These tests never run a model turn.
struct NoModel;

#[async_trait]
impl LlmProvider for NoModel {
    fn name(&self) -> &str {
        "none"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, _request: CompletionRequest) -> CompletionStream {
        unreachable!("the HTTP tests make no model calls")
    }
}

struct Harness {
    _db: TestDb,
    app: Arc<App>,
    apps: http::Local,
    pulse: Pulse,
    /// The account `phone()` signs into.
    user: String,
}

impl Harness {
    async fn new() -> Self {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let app = App::new(
            store,
            Arc::new(NoModel),
            AgentLlmConfig::new("openai:test".parse().expect("a valid model")),
            ServerTools::default(),
            ModelChoices::only("openai:test".parse().expect("a valid model")),
        );
        let pulse = Pulse::manual();
        let apps = http::Local::start(app.clone(), &SecretKey::generate(), pulse.clone())
            .await
            .expect("the apps' HTTP starts");
        let user = app
            .sign_in(&phone().public())
            .await
            .expect("signing in")
            .expect("a new key is never revoked")
            .user
            .id()
            .to_owned();
        Self {
            _db: db,
            app,
            apps,
            pulse,
            user,
        }
    }

    fn client(&self) -> Client {
        Client::new(self.apps.url(), phone()).expect("a client")
    }

    fn notify(&self, title: &str) {
        let notice = UserNotice {
            user: self.user.clone(),
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
            to: Reach::Everyone,
        };
        self.app
            .notifications
            .send(notice)
            .expect("the notice log listens");
    }
}

fn phone() -> SecretKey {
    SecretKey::from_bytes(&[3; 32])
}

async fn next(events: &mut Events) -> AppEvent {
    timeout(Duration::from_secs(20), events.next())
        .await
        .expect("an event in time")
        .expect("the stream reads")
        .expect("the stream stays open")
}

/// Reads the `Resume` and `Updated` a stream opens with; returns where it
/// resumes from.
async fn opened(events: &mut Events) -> i64 {
    let AppEvent::Resume { after } = next(events).await else {
        panic!("a stream opens with Resume");
    };
    assert!(matches!(next(events).await, AppEvent::Updated { .. }));
    after
}

/// The id and title of the next notice, skipping updates.
async fn next_notice(events: &mut Events) -> (i64, String) {
    loop {
        match next(events).await {
            AppEvent::Notice { id, notice } => return (id, notice.title),
            AppEvent::Updated { .. } => {}
            other => panic!("expected a notice, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_request_over_http_gets_its_reply() {
    let h = Harness::new().await;
    let c = h.client();
    c.call(&Health {}).await.expect("health answers");
    let sessions = c.call(&ListSessions {}).await.expect("sessions answer");
    assert!(sessions.is_empty());
    assert!(
        !c.server_info().await.expect("server info").login,
        "a server without [login] says so"
    );
}

#[tokio::test]
async fn the_events_stream_delivers_a_notice() {
    let h = Harness::new().await;
    let c = h.client();
    let mut events = c.events(None).await.expect("the stream opens");
    opened(&mut events).await;
    h.notify("Tea is ready");
    let (id, title) = next_notice(&mut events).await;
    assert_eq!(title, "Tea is ready");
    c.call(&AckNotice { id }).await.expect("the ack is taken");
}

#[tokio::test]
async fn a_stream_opened_again_resumes_after_the_last_notice() {
    let h = Harness::new().await;
    let c = h.client();
    let mut events = c.events(None).await.expect("the stream opens");
    opened(&mut events).await;
    h.notify("first");
    let (first, _) = next_notice(&mut events).await;
    drop(events);

    // Sent while the app is away, read when it comes back.
    h.notify("while away");
    h.notify("also away");
    let mut events = c.events(Some(first)).await.expect("the stream opens again");
    assert_eq!(opened(&mut events).await, first);
    assert_eq!(next_notice(&mut events).await.1, "while away");
    let (last, title) = next_notice(&mut events).await;
    assert_eq!(title, "also away");

    // From now, it waits for what is new: nothing old comes again.
    let mut events = c.events(None).await.expect("the stream opens again");
    assert_eq!(opened(&mut events).await, last);
    h.notify("new");
    assert_eq!(next_notice(&mut events).await.1, "new");
}

#[tokio::test]
async fn open_streams_get_heartbeats() {
    let h = Harness::new().await;
    let c = h.client();
    let mut events = c.events(None).await.expect("the stream opens");
    opened(&mut events).await;
    for _ in 0..2 {
        h.pulse.beat();
        assert!(matches!(next(&mut events).await, AppEvent::Heartbeat {}));
    }
}
