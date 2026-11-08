use super::*;
use crate::store::test_db::TestDb;

fn import<'a>(project: &'a str, endpoint: &'a str, folder: &'a str) -> NewImport<'a> {
    NewImport {
        project,
        endpoint,
        folder,
        url: "https://github.com/org/repo",
    }
}

async fn project_names(store: &Store, user: &UserKey) -> Vec<String> {
    store
        .projects(user)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.name.to_string())
        .collect()
}

#[tokio::test]
async fn an_imported_project_is_listed_once_and_stays_removed() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    store
        .save_import(&alice, import("org/a", "node-1", "/w/projects/org/a"))
        .await
        .unwrap();
    store
        .link_import(&alice, import("org/a", "node-2", "/srv/a"))
        .await
        .unwrap();
    store
        .link_import(&alice, import("org/b", "node-2", "/srv/b"))
        .await
        .unwrap();
    store
        .save_import(&alice, import("Org/A", "node-3", "/w/projects/Org/A"))
        .await
        .unwrap();
    assert_eq!(project_names(&store, &alice).await, ["org/a", "org/b"]);
    assert!(
        project_names(&store, &UserKey::for_test("bob"))
            .await
            .is_empty()
    );
    store.remove_project(&alice, "org/a").await.unwrap();
    assert_eq!(project_names(&store, &alice).await, ["org/b"]);
}

#[tokio::test]
async fn imports_from_before_get_their_project() {
    let (db, pool) = TestDb::at(74).await.unwrap();
    for (user, project, endpoint) in [
        ("alice", "org/a", "node-1"),
        ("alice", "org/a", "node-2"),
        ("alice", "ORG/A", "node-3"),
        ("alice", "org/opened", "node-1"),
        ("alice", "not a repo", "node-1"),
        ("bob", "org/a", "node-1"),
    ] {
        sqlx::query(
            "INSERT INTO project_imports (user_id, project, endpoint_id, folder, url)
                 VALUES ($1, $2, $3, '/w', 'https://github.com/org/a')",
        )
        .bind(user)
        .bind(project)
        .bind(endpoint)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO projects (id, user_id, name) VALUES ($1, 'alice', 'Org/Opened')")
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let names = project_names(&store, &alice).await;
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"Org/Opened".to_owned()), "{names:?}");
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("org/a")),
        "{names:?}"
    );
    assert_eq!(
        project_names(&store, &UserKey::for_test("bob")).await,
        ["org/a"]
    );
}
