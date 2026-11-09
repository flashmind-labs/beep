use super::*;
use crate::account::user::{User, UserKey};
use crate::store::{TestDb, test_chat, test_turn};
use uuid::Uuid;

async fn chat(store: &Store, name: &str) -> SessionRef {
    let session = test_chat(name);
    test_turn(store, &UserKey::for_test("alice"), session, "hi", 1000)
        .await
        .unwrap();
    SessionRef {
        user: "alice".into(),
        id: session,
    }
}

async fn set_updated(store: &Store, session: Uuid, ago: &str) {
    sqlx::query("UPDATE chats SET updated_at = now() - $1::interval WHERE id = $2")
        .bind(ago)
        .bind(session)
        .execute(store.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn picks_quiet_changed_sessions_and_keeps_manual_titles() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let quiet = chat(&store, "quiet").await;
    let busy = chat(&store, "busy").await;
    let renamed = chat(&store, "renamed").await;
    let old = chat(&store, "old").await;
    set_updated(&store, quiet.id, "5 minutes").await;
    set_updated(&store, renamed.id, "5 minutes").await;
    set_updated(&store, old.id, "2 days").await;
    assert!(
        User::for_test(store.clone(), "alice")
            .rename_session(renamed.id, "Mine")
            .await
            .unwrap()
    );

    let picked = store.sessions_to_title(IDLE_BEFORE_RETITLE).await.unwrap();
    assert_eq!(picked, vec![quiet.clone()]);

    // A title written after the last change is current until the next one.
    sqlx::query("UPDATE chats SET title = 'Greeting', titled_at = now() WHERE id = $1")
        .bind(quiet.id)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        store
            .sessions_to_title(IDLE_BEFORE_RETITLE)
            .await
            .unwrap()
            .is_empty()
    );

    assert!(
        !store
            .set_generated_title(&renamed, "Generated")
            .await
            .unwrap()
    );
    assert!(store.set_generated_title(&busy, "Busy one").await.unwrap());
    let titles: Vec<(String, String)> = User::for_test(store.clone(), "alice")
        .sessions()
        .await
        .unwrap()
        .into_iter()
        .map(|s| (s.id, s.title))
        .collect();
    assert!(titles.contains(&(renamed.id.to_string(), "Mine".into())));
    assert!(titles.contains(&(busy.id.to_string(), "Busy one".into())));
    assert!(titles.contains(&(quiet.id.to_string(), "Greeting".into())));
    assert!(titles.contains(&(old.id.to_string(), "hi".into())));
}

#[tokio::test]
async fn deletes_sessions_in_bulk() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();

    for id in ["a", "b", "c"] {
        chat(&store, id).await;
    }

    let deleted = User::for_test(store.clone(), "alice")
        .delete_sessions(&[test_chat("a"), test_chat("c"), test_chat("missing")])
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    let left: Vec<String> = User::for_test(store.clone(), "alice")
        .sessions()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(left, vec![test_chat("b").to_string()]);
}
