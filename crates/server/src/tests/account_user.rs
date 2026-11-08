use super::*;
use crate::apps::feeds::mail::Mailbox;
use crate::store::TestDb;

#[tokio::test]
async fn a_claimed_feed_cannot_be_claimed_again_until_released() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = User::for_test(store, "alice");
    let claim = user.claim_feed(Feed::Slack).expect("feed is free");
    assert!(user.claim_feed(Feed::Slack).is_none());
    assert!(user.claim_feed(Feed::Mail(Mailbox::Gmail)).is_some());
    assert!(user.claim_feed(Feed::Slack).is_none());
    drop(claim);
    assert!(user.claim_feed(Feed::Slack).is_some());
}

#[tokio::test]
async fn actions_are_kept_newest_first_per_user() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = User::for_test(store.clone(), "alice");
    let bob = User::for_test(store, "bob");
    alice
        .record_actions(&["Gmail: send email".into()])
        .await
        .unwrap();
    alice
        .record_actions(&["Calendar: create event".into()])
        .await
        .unwrap();
    let done = alice.recent_actions(10).await.unwrap();
    let done: Vec<&str> = done.iter().map(|row| row.description.as_str()).collect();
    assert_eq!(done, ["Calendar: create event", "Gmail: send email"]);
    assert!(bob.recent_actions(10).await.unwrap().is_empty());
}
