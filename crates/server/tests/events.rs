//! Account events through the whole pipeline: a trigger payload and a
//! polled GitHub notification are stored once and triaged by a sub-agent (a
//! stub that decides by the events' text). Only an inform marked now wakes
//! the main agent, a later one waits for the next wake, an ignore is only
//! logged, and a failed triage passes on just the urgent-looking events.

use async_trait::async_trait;
use beep_server::{
    app::{App, ServerTools, UserNotice},
    events::{self, Accepted, Limits, Settings, Topic, Urgency, Verdict, normalize},
    models::ModelChoices,
    store::{EventState, TestDb},
    subagents::Pool,
    user::User,
};
use chrono::{SecondsFormat, Utc};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role,
    StreamEvent,
};
use iroh::SecretKey;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::broadcast,
    time::{sleep, timeout},
};
use uuid::Uuid;

/// A model that plays both agents. As the triage agent it passes a review
/// request on now and a newsletter for later, fails on mail marked BROKEN or
/// a failed build, and ignores the rest. As the main agent it notifies on
/// what triage passed on and on a failed build handed over directly.
struct Brain;

/// What the triage stub writes about a review request.
const REVIEW_SUMMARY: &str = "Ana asked the user to review PR #42 on org/repo.";

#[async_trait]
impl LlmProvider for Brain {
    fn name(&self) -> &str {
        "test"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        let system = &request.messages[0].content;
        if system.starts_with("You keep the long-term memory") {
            return text(r#"{"ops":[]}"#);
        }
        if system.starts_with("You name chats") {
            return text("Events");
        }
        if system.starts_with("You triage events") {
            return triage(&request);
        }
        let after_tool = request
            .messages
            .last()
            .is_some_and(|message| message.role == Role::Tool);
        if after_tool {
            return text("Ana asked you to review PR #42 on org/repo. Want me to review it?");
        }
        let asked: String = request
            .messages
            .iter()
            .filter(|message| message.role == Role::User)
            .map(|message| message.content.clone())
            .collect();
        if asked.contains("From your event triage agent") || asked.contains("ci failed") {
            return call("notify_user", json!({}));
        }
        text("Nothing here needs the user.")
    }
}

/// The triage stub's one call.
fn triage(request: &CompletionRequest) -> CompletionStream {
    let batch: String = request
        .messages
        .iter()
        .filter(|message| message.role == Role::User)
        .map(|message| message.content.clone())
        .collect();
    let id = first_id(&batch).unwrap_or(0);
    if batch.contains("BROKEN") || batch.contains("ci failed") {
        return text("I am not sure.");
    }
    if batch.contains("review requested") {
        return call(
            "inform_beep",
            json!({"summary": REVIEW_SUMMARY, "why": "A person is waiting.", "urgency": "now", "refs": [id]}),
        );
    }
    if batch.contains("Newsletter") {
        return call(
            "inform_beep",
            json!({"summary": "A newsletter came in.", "why": "The user reads it.", "urgency": "later", "refs": [id]}),
        );
    }
    call("ignore", json!({"reason": "Nothing the user needs."}))
}

fn text(reply: &str) -> CompletionStream {
    let reply = reply.to_owned();
    Box::pin(async_stream::stream! {
        yield Ok(StreamEvent::ContentDelta(reply));
        yield Ok(StreamEvent::Finished(FinishReason::Stop));
    })
}

fn call(name: &str, arguments: Value) -> CompletionStream {
    let name = name.to_owned();
    Box::pin(async_stream::stream! {
        yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name });
        yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: arguments.to_string() });
        yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
    })
}

/// The first `[#id]` in the woken message.
fn first_id(text: &str) -> Option<i64> {
    let start = text.find("[#")? + 2;
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

struct Harness {
    _db: TestDb,
    app: Arc<App>,
    /// An admin, who gets events.
    alice: Arc<User>,
    /// Not an admin.
    bob: Arc<User>,
}

impl Harness {
    async fn new() -> Self {
        Self::with_limits(Settings::default().limits).await
    }

    async fn with_limits(limits: Limits) -> Self {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (SecretKey::generate(), SecretKey::generate());
        let model = "openai:test".parse().unwrap();
        let models = ModelChoices::resolve(&Brain, model, Vec::new(), &[alice.public()]).await;
        let app = App::new(
            store,
            Arc::new(Brain),
            AgentLlmConfig::new("openai:test".parse().unwrap()),
            ServerTools::default(),
            models,
        );
        events::start(
            &app,
            Settings {
                batch_window: Duration::ZERO,
                tick: Duration::from_millis(20),
                digest_after: Duration::ZERO,
                sweep_every: Duration::from_millis(100),
                poll: false,
                limits,
                ..Settings::default()
            },
        );
        let sign_in = async |key: &SecretKey| {
            app.sign_in(&key.public())
                .await
                .unwrap()
                .expect("a new key is never revoked")
                .user
        };
        let (alice, bob) = (sign_in(&alice).await, sign_in(&bob).await);
        Self {
            _db: db,
            app,
            alice,
            bob,
        }
    }
}

/// A Composio V3 trigger payload for a new Gmail message to `user`.
fn gmail_trigger(user: &str, id: &str, sender: &str, subject: &str) -> Value {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    json!({
        "id": format!("log_{id}"),
        "timestamp": now,
        "type": "composio.trigger.message",
        "metadata": {
            "log_id": format!("log_{id}"),
            "trigger_slug": "GMAIL_NEW_GMAIL_MESSAGE",
            "trigger_id": "ti_1",
            "connected_account_id": "ca_1",
            "auth_config_id": "ac_1",
            "user_id": user
        },
        "data": {
            "message_id": id,
            "thread_id": format!("t_{id}"),
            "sender": sender,
            "subject": subject,
            "label_ids": ["INBOX", "UNREAD"],
            "message_timestamp": now
        }
    })
}

/// A notification as `GITHUB_LIST_NOTIFICATIONS` returns it.
fn github_notification(reason: &str, title: &str) -> Value {
    json!({
        "id": "9001",
        "reason": reason,
        "unread": true,
        "updated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "repository": {"full_name": "org/repo"},
        "subject": {
            "title": title,
            "type": "PullRequest",
            "url": "https://api.github.com/repos/org/repo/pulls/42"
        }
    })
}

/// The next notice with a title, the kind a finished woken run sends.
async fn next_notice(notices: &mut broadcast::Receiver<UserNotice>) -> UserNotice {
    timeout(Duration::from_secs(20), async {
        loop {
            let notice = notices.recv().await.expect("the notices go on");
            if !notice.notice.title.is_empty() {
                return notice;
            }
        }
    })
    .await
    .expect("a notice arrives")
}

/// Waits until `user` has `count` events in `state`. Polls the store,
/// because the pipeline gives no signal when it moves events on.
async fn events_in(user: &User, state: EventState, count: usize) -> Vec<i64> {
    timeout(Duration::from_secs(20), async {
        loop {
            let rows = user.events_in(state, Duration::ZERO, 50).await.unwrap();
            if rows.len() >= count {
                return rows.iter().map(|row| row.id).collect();
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{count} events reach {state:?}"))
}

/// Waits until `user` has no sub-agent running, so its triage is settled.
async fn triaged(h: &Harness, user: &User) {
    timeout(Duration::from_secs(20), async {
        while h.app.subagents.running(Pool::Triage, user.id()) > 0 {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the triage ends");
}

/// The verdict and urgency logged for event `id`.
async fn verdict_of(user: &User, id: i64) -> (Verdict, Option<Urgency>, bool) {
    let recent = user
        .recent_events(Utc::now() - chrono::Duration::hours(1), 50)
        .await
        .unwrap();
    let event = recent
        .iter()
        .find(|recent| recent.event.id == id)
        .expect("the event is listed");
    let triage = event.triage.as_ref().expect("the event was triaged");
    (triage.verdict, triage.urgency, triage.delivered)
}

#[tokio::test]
async fn an_inform_now_wakes_the_agent_with_the_triage_summary() {
    let h = Harness::new().await;
    let mut notices = h.app.notifications.subscribe();
    let alice = &h.alice;

    // A review request found by the GitHub poll is passed on now.
    let review = normalize::from_github_notification(&github_notification(
        "review_requested",
        "Add caching",
    ))
    .unwrap();
    events::submit(&h.app, alice.id(), review.clone()).await;
    let notice = next_notice(&mut notices).await;
    assert_eq!(notice.user, alice.id());
    assert_eq!(notice.notice.title, REVIEW_SUMMARY);
    assert!(notice.notice.body.contains("Want me to review it?"));
    let ids = events_in(alice, EventState::Triaged, 1).await;
    assert_eq!(
        verdict_of(alice, ids[0]).await,
        (Verdict::Inform, Some(Urgency::Now), true)
    );

    // The same happening again, by trigger or by poll, is stored once.
    assert_eq!(
        events::accept(&h.app, alice, review).await.unwrap(),
        Accepted::Duplicate
    );
    // A topic the user turned off is dropped.
    alice.set_event_topic(Topic::GithubCi, false).await.unwrap();
    let failed = normalize::from_github_notification(&github_notification(
        "ci_activity",
        "CI workflow run failed for main branch",
    ))
    .unwrap();
    assert_eq!(
        events::accept(&h.app, alice, failed.clone()).await.unwrap(),
        Accepted::Unsubscribed
    );
    // Only admins get events for now.
    assert_eq!(
        events::accept(&h.app, &h.bob, failed).await.unwrap(),
        Accepted::Off
    );
}

#[tokio::test]
async fn an_ignore_is_logged_and_a_later_waits_for_the_next_wake() {
    let h = Harness::new().await;
    let mut notices = h.app.notifications.subscribe();
    let alice = &h.alice;

    // Lunch mail is ignored: logged, no chat.
    let payload = gmail_trigger(alice.external_id(), "m1", "Ana <ana@example.com>", "Lunch");
    events::deliver(&h.app, &payload).await;
    let ids = events_in(alice, EventState::Triaged, 1).await;
    triaged(&h, alice).await;
    assert_eq!(
        verdict_of(alice, ids[0]).await,
        (Verdict::Ignore, None, false)
    );
    // A trigger and a poll of the same email are one event.
    let message = normalize::TriggerMessage::parse(&payload).unwrap();
    let again = normalize::from_trigger(&message).unwrap();
    assert_eq!(
        events::accept(&h.app, alice, again).await.unwrap(),
        Accepted::Duplicate
    );

    // A newsletter is kept for later: logged, nothing woken.
    let payload = gmail_trigger(
        alice.external_id(),
        "m2",
        "News <news@example.com>",
        "Newsletter: this week",
    );
    events::deliver(&h.app, &payload).await;
    let ids = events_in(alice, EventState::Triaged, 2).await;
    triaged(&h, alice).await;
    let newsletter = *ids.iter().max().unwrap();
    assert_eq!(
        verdict_of(alice, newsletter).await,
        (Verdict::Inform, Some(Urgency::Later), false)
    );
    assert!(alice.sessions().await.unwrap().is_empty());
    assert_eq!(
        alice
            .undelivered_informs(Utc::now() - chrono::Duration::hours(1), 10)
            .await
            .unwrap()
            .len(),
        1
    );

    // The next inform now carries it along.
    let review = normalize::from_github_notification(&github_notification(
        "review_requested",
        "Add caching",
    ))
    .unwrap();
    events::submit(&h.app, alice.id(), review).await;
    next_notice(&mut notices).await;
    assert_eq!(
        verdict_of(alice, newsletter).await,
        (Verdict::Inform, Some(Urgency::Later), true)
    );
}

#[tokio::test]
async fn a_failed_triage_passes_on_only_urgent_looking_events() {
    let h = Harness::new().await;
    let mut notices = h.app.notifications.subscribe();
    let alice = &h.alice;

    // Mail the triage stub chokes on looks routine: logged and dropped.
    let payload = gmail_trigger(alice.external_id(), "m3", "Ana <ana@example.com>", "BROKEN");
    events::deliver(&h.app, &payload).await;
    let ids = events_in(alice, EventState::Triaged, 1).await;
    triaged(&h, alice).await;
    assert_eq!(
        verdict_of(alice, ids[0]).await,
        (Verdict::Failed, None, false)
    );
    assert!(alice.sessions().await.unwrap().is_empty());

    // A failed build looks urgent, so the main agent gets it directly.
    let failed = normalize::from_github_notification(&github_notification(
        "ci_activity",
        "CI workflow run failed for main branch",
    ))
    .unwrap();
    events::submit(&h.app, alice.id(), failed).await;
    let notice = next_notice(&mut notices).await;
    assert!(notice.notice.title.contains("CI workflow run failed"));
    let woken = events_in(alice, EventState::Woken, 1).await;
    assert_eq!(verdict_of(alice, woken[0]).await.0, Verdict::Failed);
}

#[tokio::test]
async fn limits_hold_triages_and_wakes_back() {
    let h = Harness::with_limits(Limits {
        events_per_hour: 200,
        wakes_per_hour: 0,
        triages_per_hour: 1,
    })
    .await;
    let alice = &h.alice;

    // With no wake left, an inform now waits like a later one.
    let review = normalize::from_github_notification(&github_notification(
        "review_requested",
        "Add caching",
    ))
    .unwrap();
    events::submit(&h.app, alice.id(), review).await;
    let ids = events_in(alice, EventState::Triaged, 1).await;
    triaged(&h, alice).await;
    assert_eq!(
        verdict_of(alice, ids[0]).await,
        (Verdict::Inform, Some(Urgency::Now), false)
    );
    assert!(alice.sessions().await.unwrap().is_empty());

    // With no triage left this hour, new events stay pending.
    let payload = gmail_trigger(alice.external_id(), "m4", "Ana <ana@example.com>", "Lunch");
    events::deliver(&h.app, &payload).await;
    events_in(alice, EventState::Pending, 1).await;
    sleep(Duration::from_millis(300)).await;
    assert_eq!(
        alice
            .events_in(EventState::Pending, Duration::ZERO, 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn duplicates_do_not_count_toward_the_hourly_limit() {
    let h = Harness::with_limits(Limits {
        events_per_hour: 2,
        wakes_per_hour: 0,
        triages_per_hour: 0,
    })
    .await;
    let alice = &h.alice;
    let mail = |id: &str| {
        let payload = gmail_trigger(alice.external_id(), id, "Ana <ana@example.com>", "Lunch");
        let message = normalize::TriggerMessage::parse(&payload).unwrap();
        normalize::from_trigger(&message).unwrap()
    };
    assert!(matches!(
        events::accept(&h.app, alice, mail("d1")).await.unwrap(),
        Accepted::Stored(_)
    ));
    for _ in 0..3 {
        assert_eq!(
            events::accept(&h.app, alice, mail("d1")).await.unwrap(),
            Accepted::Duplicate
        );
    }
    assert!(matches!(
        events::accept(&h.app, alice, mail("d2")).await.unwrap(),
        Accepted::Stored(_)
    ));
    assert_eq!(
        events::accept(&h.app, alice, mail("d3")).await.unwrap(),
        Accepted::Limited
    );
}
