use super::*;
use crate::store::{test_db::TestDb, test_turn};

#[tokio::test]
async fn allows_are_kept_per_chat() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let (chat, other) = (Uuid::new_v4(), Uuid::new_v4());
    test_turn(&store, &alice, chat, "hi", 1).await.unwrap();
    test_turn(&store, &alice, other, "hi", 2).await.unwrap();
    let send = ChatAllow {
        app: "gmail".into(),
        tool: "GMAIL_SEND_EMAIL".into(),
    };
    store.allow_in_chat(&alice, chat, &send).await.unwrap();
    // Allowing twice keeps one.
    store.allow_in_chat(&alice, chat, &send).await.unwrap();
    assert_eq!(store.chat_allows(&alice, chat).await.unwrap(), [send]);
    assert!(store.chat_allows(&alice, other).await.unwrap().is_empty());
    let bob = UserKey::for_test("bob");
    assert!(store.chat_allows(&bob, chat).await.unwrap().is_empty());
}
