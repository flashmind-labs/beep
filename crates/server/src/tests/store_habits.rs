use super::*;

#[test]
fn streak_counts_back_from_today_or_yesterday() {
    let day = |d: u32| NaiveDate::from_ymd_opt(2026, 3, d).unwrap();
    assert_eq!(streak(&[], day(10)), 0);
    assert_eq!(streak(&[day(10), day(9), day(7)], day(10)), 2);
    assert_eq!(streak(&[day(9), day(8)], day(10)), 2);
    assert_eq!(streak(&[day(8), day(7)], day(10)), 0);
}

#[tokio::test]
async fn status_covers_exactly_the_days_asked_for() {
    use crate::store::TestDb;

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    store.create_habit(&alice, "Run", "daily").await.unwrap();
    store.log_habit(&alice, "Run", 1.0, None).await.unwrap();
    store.log_habit(&alice, "Run", 1.0, None).await.unwrap();
    sqlx::query(
        "UPDATE habit_logs SET logged_at = logged_at - interval '1 day'
             WHERE id = (SELECT id FROM habit_logs LIMIT 1)",
    )
    .execute(store.pool())
    .await
    .unwrap();

    let today = store.habit_status(&alice, "Run", 1).await.unwrap();
    assert_eq!(today["logged_days"], 1);
    assert_eq!(today["completion_rate"], 1.0);
    let two = store.habit_status(&alice, "Run", 2).await.unwrap();
    assert_eq!(two["logged_days"], 2);
    assert_eq!(two["completion_rate"], 1.0);
}

#[tokio::test]
async fn names_count_characters_not_bytes() {
    use crate::store::TestDb;

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    store
        .create_habit(&alice, &"é".repeat(200), "daily")
        .await
        .unwrap();
    assert!(
        store
            .create_habit(&alice, &"é".repeat(201), "daily")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_archived_habit_comes_back_by_name() {
    use crate::store::TestDb;

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let id = store.create_habit(&alice, "Run", "daily").await.unwrap();
    assert!(store.create_habit(&alice, "Run", "daily").await.is_err());
    assert!(store.archive_habit(&alice, "Run").await.unwrap());
    let again = store.create_habit(&alice, "Run", "weekly").await.unwrap();
    assert_eq!(again, id);
    assert_eq!(store.list_habits(&alice).await.unwrap().len(), 1);
}
