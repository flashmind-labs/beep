use super::*;
use crate::store::test_db::TestDb;

fn row(project: &str, endpoint: &str, folder: &str) -> SyncRow {
    SyncRow {
        project: project.into(),
        endpoint: endpoint.into(),
        folder: folder.into(),
    }
}

#[tokio::test]
async fn pinning_a_project_stops_it_everywhere() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let other = UserKey::for_test("bob");
    store
        .set_project_sync(&user, &row("game", "a", "/srv/one"))
        .await
        .unwrap();
    store
        .set_project_sync(&user, &row("game", "a", "/srv/game"))
        .await
        .unwrap();
    store
        .set_project_sync(&user, &row("game", "b", "/srv/game"))
        .await
        .unwrap();
    store
        .set_project_sync(&user, &row("site", "a", "/srv/site"))
        .await
        .unwrap();
    store
        .set_project_sync(&other, &row("game", "c", "/srv/game"))
        .await
        .unwrap();
    assert_eq!(
        store.project_syncs(&user).await.unwrap(),
        [
            row("game", "a", "/srv/game"),
            row("game", "b", "/srv/game"),
            row("site", "a", "/srv/site"),
        ]
    );
    assert!(store.remove_project_sync(&user, "site", "a").await.unwrap());
    assert!(!store.remove_project_sync(&user, "site", "a").await.unwrap());

    store.pin_project(&user, "game", true).await.unwrap();
    assert!(store.project_syncs(&user).await.unwrap().is_empty());
    assert_eq!(store.pinned_projects(&user).await.unwrap(), ["game"]);
    assert_eq!(store.project_syncs(&other).await.unwrap().len(), 1);
    store.pin_project(&user, "game", false).await.unwrap();
    assert!(store.pinned_projects(&user).await.unwrap().is_empty());
}
