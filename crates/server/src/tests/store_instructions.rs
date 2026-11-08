use super::*;
use crate::store::test_db::TestDb;

#[tokio::test]
async fn versions_count_up_and_a_stale_base_is_refused() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    assert!(
        store
            .instructions_versions(&user, 1)
            .await
            .unwrap()
            .is_empty()
    );
    let Saved::Made(first) = store
        .save_instructions(&user, None, "- Write plainly.", "")
        .await
        .unwrap()
    else {
        panic!("the first save is made");
    };
    assert_eq!(first.version, 1);
    assert_eq!(
        store
            .save_instructions(&user, None, "other", "")
            .await
            .unwrap(),
        Saved::Stale(1)
    );
    let Saved::Made(second) = store
        .save_instructions(&user, Some(1), "- Keep it short.", "Restored version 1")
        .await
        .unwrap()
    else {
        panic!("a save on the newest is made");
    };
    assert_eq!(second.version, 2);
    let listed = store.instructions_versions(&user, 50).await.unwrap();
    assert_eq!(listed.iter().map(|v| v.version).collect::<Vec<_>>(), [2, 1]);
    assert_eq!(listed[0].note, "Restored version 1");
    let bob = UserKey::for_test("bob");
    assert!(
        store
            .instructions_versions(&bob, 50)
            .await
            .unwrap()
            .is_empty()
    );
}
