use super::*;
use crate::{
    account::user::UserKey,
    store::{TestDb, test_chat, test_turn},
};
use chrono::TimeDelta;

#[tokio::test]
async fn keeps_each_watchers_place() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let chat = test_chat("watched");
    test_turn(&store, &alice, chat, "hello there", 1_000)
        .await
        .unwrap();
    let last = test_turn(&store, &alice, chat, "and again", 2_000)
        .await
        .unwrap();
    test_turn(
        &store,
        &UserKey::for_test("bob"),
        test_chat("bobs"),
        "hi",
        3_000,
    )
    .await
    .unwrap();

    let long_ago = Utc::now() - TimeDelta::days(1);
    let chats = store.watched_chats(long_ago).await.unwrap();
    assert_eq!(chats.len(), 2);
    let bobs = chats.iter().find(|found| found.user == "bob").unwrap();
    assert_eq!(bobs.chat, test_chat("bobs"));
    let alices = chats.iter().find(|found| found.user == "alice").unwrap();
    assert_eq!(alices.chat, chat);
    // The reply comes after the message that opened the last turn.
    assert!(alices.last_seq > last);
    assert!(!alices.running);
    let none = store.watched_chats(Utc::now() + TimeDelta::minutes(1));
    assert!(none.await.unwrap().is_empty());
    assert!(store.watched_chat("bob", chat).await.unwrap().is_none());

    let at = Utc::now();
    store
        .mark_watched(chat, WatcherKind::Memory, last, at)
        .await
        .unwrap();
    assert_eq!(
        store
            .watch_failed(chat, WatcherKind::Skills, at)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .watch_failed(chat, WatcherKind::Skills, at)
            .await
            .unwrap(),
        2
    );
    // An older place never moves it back.
    store
        .mark_watched(chat, WatcherKind::Memory, 1, at)
        .await
        .unwrap();
    let states = store.watcher_states(&[chat]).await.unwrap();
    let memory = &states[&(chat, WatcherKind::Memory)];
    assert_eq!(memory.reviewed_seq, last);
    assert_eq!(memory.failures, 0);
    let skills = &states[&(chat, WatcherKind::Skills)];
    assert_eq!(skills.reviewed_seq, 0);
    assert_eq!(skills.failures, 2);
    assert!(skills.failed_at.is_some());
    assert!(!states.contains_key(&(chat, WatcherKind::Improvement)));

    // Reading clears the failures.
    store
        .mark_watched(chat, WatcherKind::Skills, last, at)
        .await
        .unwrap();
    let states = store.watcher_states(&[chat]).await.unwrap();
    assert_eq!(states[&(chat, WatcherKind::Skills)].failures, 0);
    assert_eq!(states[&(chat, WatcherKind::Skills)].failed_at, None);

    let read = store
        .chat_messages_after(chat, last - 1, DateTime::UNIX_EPOCH, 10)
        .await
        .unwrap();
    assert_eq!(read.first().map(|m| m.id), Some(last));
    let later = store
        .chat_messages_after(chat, 0, Utc::now() + TimeDelta::minutes(1), 10)
        .await
        .unwrap();
    assert!(later.is_empty());
}
