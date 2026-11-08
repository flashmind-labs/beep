use super::*;
use crate::{
    routines::events::{DedupeKey, Source},
    store::TestDb,
};
use serde_json::json;

fn mail(id: &str, source: Source) -> AppEvent {
    AppEvent {
        app: EventApp::Gmail,
        kind: EventKind::NewMail,
        key: DedupeKey::new(&["gmail", id]),
        summary: format!("From someone: {id}"),
        detail: None,
        ids: json!({"message_id": id}),
        raw: json!({"id": id}),
        source,
        occurred: Utc::now(),
    }
}

#[tokio::test]
async fn the_same_happening_is_stored_once_however_it_arrives() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    let first = store
        .insert_event(&alice, &mail("m1", Source::Trigger))
        .await;
    assert!(first.unwrap().is_some());
    let again = store.insert_event(&alice, &mail("m1", Source::Poll)).await;
    assert!(again.unwrap().is_none());
    // Another user's copy is their own.
    let theirs = store.insert_event(&bob, &mail("m1", Source::Poll)).await;
    assert!(theirs.unwrap().is_some());

    let pending = store
        .events_in(&alice, EventState::Pending, Duration::ZERO, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind, EventKind::NewMail);
    assert_eq!(pending[0].ids["message_id"], "m1");

    let ids = [pending[0].id];
    let moved = store
        .move_events(
            &alice,
            &ids,
            EventState::Pending,
            EventState::Woken,
            Some("r1"),
        )
        .await
        .unwrap();
    assert_eq!(moved, 1);
    let moved = store
        .move_events(&bob, &ids, EventState::Woken, EventState::Held, None)
        .await
        .unwrap();
    assert_eq!(moved, 0, "another user's events stay");
    let moved = store
        .move_events(&alice, &ids, EventState::Pending, EventState::Held, None)
        .await
        .unwrap();
    assert_eq!(moved, 0, "only events still pending move");
}

#[tokio::test]
async fn a_triage_that_never_settled_goes_back_in_the_queue() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let id = store
        .insert_event(&alice, &mail("m1", Source::Trigger))
        .await
        .unwrap()
        .unwrap();
    store
        .move_events(
            &alice,
            &[id],
            EventState::Pending,
            EventState::Triaging,
            None,
        )
        .await
        .unwrap();
    let hour = Duration::from_secs(60 * 60);
    assert_eq!(
        store.requeue_stale_triaging(&alice, hour).await.unwrap(),
        0,
        "a triage still in its time is left alone"
    );
    assert_eq!(
        store
            .requeue_stale_triaging(&UserKey::for_test("bob"), Duration::ZERO)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .requeue_stale_triaging(&alice, Duration::ZERO)
            .await
            .unwrap(),
        1
    );
    let pending = store
        .events_in(&alice, EventState::Pending, Duration::ZERO, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
}

/// Logs an inform about a new event of `user`'s, as a triage does.
async fn inform(store: &Store, user: &UserKey, mail_id: &str, summary: &str) -> i64 {
    let id = store
        .insert_event(user, &mail(mail_id, Source::Trigger))
        .await
        .unwrap()
        .unwrap();
    store
        .move_events(user, &[id], EventState::Pending, EventState::Triaging, None)
        .await
        .unwrap();
    let triage = NewTriage {
        verdict: Verdict::Inform,
        urgency: Some(Urgency::Later),
        summary: Some(summary),
        reason: "worth knowing",
        refs: &[id],
    };
    store.insert_triage(user, &triage, &[id]).await.unwrap()
}

#[tokio::test]
async fn waiting_updates_are_the_users_newest_first_until_delivered() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    let first = inform(&store, &alice, "m1", "The build is ready").await;
    let second = inform(&store, &alice, "m2", "The invoice came").await;
    let theirs = inform(&store, &bob, "m3", "Bob's news").await;
    let since = Utc::now() - chrono::Duration::days(2);

    let waiting = store.waiting_updates(&alice, since, 20).await.unwrap();
    let ids: Vec<i64> = waiting.iter().map(|row| row.id).collect();
    assert_eq!(ids, [second, first]);
    assert_eq!(waiting[0].summary, "The invoice came");
    assert_eq!(waiting[0].app, Some(EventApp::Gmail));
    assert_eq!(
        store.waiting_updates(&alice, since, 1).await.unwrap().len(),
        1
    );
    assert!(
        store
            .waiting_updates(&alice, Utc::now(), 20)
            .await
            .unwrap()
            .is_empty(),
        "older than `since` is left out"
    );

    // Another user's id is not alice's to dismiss.
    store
        .mark_delivered(&alice, &[first, theirs], None)
        .await
        .unwrap();
    let waiting = store.waiting_updates(&alice, since, 20).await.unwrap();
    assert_eq!(
        waiting.iter().map(|row| row.id).collect::<Vec<_>>(),
        [second]
    );
    let bobs = store.waiting_updates(&bob, since, 20).await.unwrap();
    assert_eq!(bobs.iter().map(|row| row.id).collect::<Vec<_>>(), [theirs]);
}
