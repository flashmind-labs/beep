use super::*;
use crate::{
    account::user::UserKey,
    routines::events::{AppEvent, DedupeKey, EventApp, EventKind, Source, Urgency, Verdict},
    store::{EventState, NewTriage, Store, StoredBody, TestDb},
};
use chrono::Utc;
use serde_json::json;

/// An update for `user` about one new mail, as the triage agent logs it.
async fn inform(store: &Store, user: &UserKey, summary: &str) -> i64 {
    let event = AppEvent {
        app: EventApp::Gmail,
        kind: EventKind::NewMail,
        key: DedupeKey::new(&["gmail", "m1"]),
        summary: "From Ana: Review please".to_owned(),
        detail: None,
        ids: json!({"message_id": "m1"}),
        raw: json!({"id": "m1", "subject": "Review please"}),
        source: Source::Trigger,
        occurred: Utc::now(),
    };
    let id = store.insert_event(user, &event).await.unwrap().unwrap();
    store
        .move_events(user, &[id], EventState::Pending, EventState::Triaging, None)
        .await
        .unwrap();
    let triage = NewTriage {
        verdict: Verdict::Inform,
        urgency: Some(Urgency::Later),
        summary: Some(summary),
        reason: "A person is waiting.",
        refs: &[id],
    };
    store.insert_triage(user, &triage, &[id]).await.unwrap()
}

#[tokio::test]
async fn an_update_opens_one_chat_that_starts_with_beep() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let key = UserKey::for_test("alice");
    let alice = User::for_test(store.clone(), "alice");
    let summary = "Ana asked you to review PR 42 by Friday.";
    let id = inform(&store, &key, summary).await;

    let chat = open(&alice, id).await.unwrap().expect("alice's update");
    assert_eq!(open(&alice, id).await.unwrap(), Some(chat), "reopened");
    let sessions = alice.sessions().await.unwrap();
    let listed: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(listed, [chat.to_string()], "made once");

    let messages = alice.session_messages(chat).await.unwrap().unwrap();
    let bodies: Vec<&StoredBody> = messages.iter().map(|m| &m.body).collect();
    assert_eq!(
        bodies,
        [&StoredBody::Assistant {
            content: summary.to_owned()
        }]
    );
    // Beep answers a reply with the event in what it reads.
    let context = store.context(&key, chat).await.unwrap().unwrap();
    assert!(
        context.conversation.entries()[0]
            .content()
            .contains("Review please")
    );
    // Opening it took it off the Updates list.
    let since = Utc::now() - chrono::Duration::days(2);
    assert!(alice.waiting_updates(since, 20).await.unwrap().is_empty());

    let bob = User::for_test(store.clone(), "bob");
    assert_eq!(open(&bob, id).await.unwrap(), None, "not bob's");
}
