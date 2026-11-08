use super::*;
use crate::store::test_db::TestDb;

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| (*p).to_owned()).collect()
}

#[tokio::test]
async fn groups_are_saved_renamed_and_forgotten_per_user() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let other = UserKey::for_test("bob");
    let first = paths(&["work"]);
    let field = store
        .save_project_group(
            &user,
            SaveGroup {
                id: None,
                name: "work",
                paths: &first,
            },
        )
        .await
        .unwrap()
        .expect("a new group is made");
    let second = paths(&["/srv/project", "work/tools"]);
    let tools = store
        .save_project_group(
            &user,
            SaveGroup {
                id: None,
                name: "tools",
                paths: &second,
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        store.project_groups(&user).await.unwrap(),
        [field.clone(), tools.clone()]
    );
    assert!(store.project_groups(&other).await.unwrap().is_empty());

    let renamed = paths(&["work", "games"]);
    let changed = store
        .save_project_group(
            &user,
            SaveGroup {
                id: Some(field.id),
                name: "Field",
                paths: &renamed,
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed.id, field.id);
    assert_eq!(changed.name, "Field");
    assert_eq!(changed.paths, renamed);
    // Another user's group is not theirs to change or forget.
    let taken = store
        .save_project_group(
            &other,
            SaveGroup {
                id: Some(field.id),
                name: "mine",
                paths: &first,
            },
        )
        .await
        .unwrap();
    assert!(taken.is_none());
    assert!(!store.delete_project_group(&other, field.id).await.unwrap());

    assert!(store.delete_project_group(&user, field.id).await.unwrap());
    assert!(!store.delete_project_group(&user, field.id).await.unwrap());
    assert_eq!(store.project_groups(&user).await.unwrap(), [tools]);
}
