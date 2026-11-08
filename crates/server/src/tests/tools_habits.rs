use super::*;
use crate::{account::user::UserKey, store::TestDb};

#[tokio::test]
async fn a_second_habit_of_the_same_name_is_a_duplicate() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store");
    let alice = UserKey::for_test("alice");
    store
        .create_habit(&alice, "Run", "daily")
        .await
        .expect("first habit");
    let again = store
        .create_habit(&alice, "Run", "weekly")
        .await
        .expect_err("same name");
    assert!(is_duplicate(&again), "{again:#}");
    let invalid = store
        .create_habit(&alice, "Swim", "hourly")
        .await
        .expect_err("bad frequency");
    assert!(!is_duplicate(&invalid));
}
