use super::*;
use crate::account::user::{User, UserKey};
use crate::store::{StoredMessage, TestDb, test_chat, test_turn};
use flashmind::core::ConversationEntry;

fn texts(conversation: &Conversation) -> Vec<&str> {
    conversation.entries().iter().map(|e| e.content()).collect()
}

async fn messages(user: &User, chat: Uuid) -> Option<Vec<String>> {
    let messages = user.session_messages(chat).await.unwrap()?;
    Some(
        messages
            .iter()
            .map(StoredMessage::text)
            .map(str::to_owned)
            .collect(),
    )
}

#[tokio::test]
async fn a_share_keeps_its_token_and_others_can_copy_it() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = User::for_test(store.clone(), "alice");
    let bob = User::for_test(store.clone(), "bob");
    let (s1, b1, b2) = (test_chat("s1"), test_chat("b1"), test_chat("b2"));
    test_turn(&store, &UserKey::for_test("alice"), s1, "hello", 1000)
        .await
        .unwrap();

    let mut copy = Conversation::new();
    copy.add(ConversationEntry::user("first"));
    assert!(bob.share_session(s1, &copy).await.unwrap().is_none());
    let first = alice.share_session(s1, &copy).await.unwrap().unwrap();
    assert_eq!(first.title, "hello");
    copy.add(ConversationEntry::assistant("second"));
    let again = alice.share_session(s1, &copy).await.unwrap().unwrap();
    assert_eq!(again.token, first.token);
    let shared = store.share(&first.token).await.unwrap().unwrap();
    assert_eq!(texts(&shared.conversation), ["first", "second"]);

    assert_eq!(
        bob.copy_share(&first.token, b1).await.unwrap(),
        CopiedShare::Copied
    );
    assert_eq!(
        bob.copy_share(&first.token, b1).await.unwrap(),
        CopiedShare::Taken
    );
    assert_eq!(
        bob.copy_share("missing", b2).await.unwrap(),
        CopiedShare::NotFound
    );
    assert_eq!(messages(&bob, b1).await.unwrap(), ["first", "second"]);
    let sessions = bob.sessions().await.unwrap();
    assert_eq!(sessions[0].title, "hello");
    assert!(bob.rename_session(b1, "mine").await.unwrap());

    alice.delete_session(s1).await.unwrap();
    assert!(store.share(&first.token).await.unwrap().is_none());
    assert!(messages(&bob, b1).await.is_some());
}

#[tokio::test]
async fn unsharing_breaks_the_link() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = User::for_test(store.clone(), "alice");
    let s1 = test_chat("s1");
    test_turn(&store, &UserKey::for_test("alice"), s1, "hello", 1000)
        .await
        .unwrap();
    let share = alice
        .share_session(s1, &Conversation::new())
        .await
        .unwrap()
        .unwrap();
    assert!(alice.unshare_session(s1).await.unwrap());
    assert!(!alice.unshare_session(s1).await.unwrap());
    assert!(store.share(&share.token).await.unwrap().is_none());
}
