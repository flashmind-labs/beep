use crate::{account::user::UserKey, store::TestDb};

#[tokio::test]
async fn updates_are_held_to_the_same_bounds_as_new_routines() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let id = store
        .create_cron(&alice, "Stretch", "0 9 * * *", "Remind me to stretch")
        .await
        .unwrap();

    assert!(
        store
            .update_cron(&alice, &id, None, Some(""), None)
            .await
            .is_err()
    );
    let long = "x".repeat(201);
    assert!(
        store
            .update_cron(&alice, &id, None, Some(&long), None)
            .await
            .is_err()
    );
    assert!(
        store
            .update_cron(&alice, &id, None, None, Some(""))
            .await
            .is_err()
    );
    let crons = store.list_crons(&alice).await.unwrap();
    assert_eq!(crons[0].label, "Stretch");
    assert_eq!(crons[0].schedule, "0 9 * * *");
    assert!(
        store
            .update_cron(&alice, &id, None, Some("Walk"), None)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn limits_count_characters_not_bytes() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let label = "é".repeat(200);
    store
        .create_cron(&alice, &label, "0 9 * * *", "Remind me")
        .await
        .unwrap();
    let label = "é".repeat(201);
    assert!(
        store
            .create_cron(&alice, &label, "0 9 * * *", "Remind me")
            .await
            .is_err()
    );
}
