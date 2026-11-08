use super::*;
use crate::store::TestDb;
use std::time::Duration;

#[tokio::test]
async fn a_new_timezone_reloads_the_scheduler() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let changed = store.crons_changed();
    let user = UserKey::for_test("alice");
    let at = |timezone: &str| Location {
        latitude: 1.0,
        longitude: 2.0,
        accuracy: 10.0,
        timezone: timezone.to_owned(),
        place: None,
        updated_at: Utc::now(),
    };
    let reloads = || async {
        tokio::time::timeout(Duration::from_millis(200), changed.notified())
            .await
            .is_ok()
    };

    store
        .update_location(&user, &at("Europe/Madrid"))
        .await
        .unwrap();
    assert!(reloads().await, "the first timezone reloads");
    store
        .update_location(&user, &at("Europe/Madrid"))
        .await
        .unwrap();
    assert!(!reloads().await, "the same timezone does not");
    store
        .update_location(&user, &at("Asia/Tokyo"))
        .await
        .unwrap();
    assert!(reloads().await, "a new timezone does");
}
