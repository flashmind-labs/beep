use super::*;
use crate::store::{CreateRun, RunStatus, TestDb, test_chat};

#[tokio::test]
async fn alerts_past_the_cap_are_deleted_with_their_chats() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let mut chats = Vec::new();

    for n in 0..=LISTED {
        let chat = test_chat(&format!("alert-{n}"));
        let made = store
            .create_run(&alice, chat, &format!("req-{n}"), "wake")
            .await
            .unwrap();
        let CreateRun::Created(run) = made else {
            panic!("the run was not created");
        };
        store
            .finish(&alice, &run, None, RunStatus::Completed)
            .await
            .unwrap();
        store
            .add_alert(&alice, chat, "Apps", "Hello")
            .await
            .unwrap();
        chats.push(chat);
    }

    let listed = store.alerts(&alice).await.unwrap();
    assert_eq!(listed.len(), LISTED as usize);
    // The oldest is gone with its chat, so it is no stray chat in a list.
    assert!(listed.iter().all(|a| a.chat != chats[0]));
    assert!(!store.in_chat(&alice, chats[0]).await.unwrap());
}
