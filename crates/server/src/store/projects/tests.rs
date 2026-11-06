use super::*;
use crate::{account::user::UserKey, projects::model::TaskState, store::TestDb};
use beep_projects::model::{RepoName, Visibility};

fn repo(name: &str) -> RepoName {
    RepoName::parse(name).expect("a valid repo name")
}

fn new_task(title: &str) -> NewTask {
    NewTask {
        title: title.into(),
        body: String::new(),
        depends_on: None,
        needs: Vec::new(),
        start: None,
    }
}

#[tokio::test]
async fn a_project_and_its_chat_are_made_once() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let made = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let again = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    assert_eq!(made.id, again.id);
    assert_eq!(made.visibility, Visibility::Public);
    assert_eq!(made.default_branch, "main");
    assert_eq!(store.projects(&user).await.unwrap().len(), 1);
    // Another user's project of the same name is theirs.
    let bob = UserKey::for_test("bob");
    let theirs = store
        .ensure_project(&bob, &repo("acme/widget"))
        .await
        .unwrap();
    assert_ne!(theirs.id, made.id);
    assert!(store.project(&bob, made.id).await.unwrap().is_none());

    let chat = store.ensure_project_agent(&user, made.id).await.unwrap();
    assert_eq!(
        store.ensure_project_agent(&user, made.id).await.unwrap(),
        chat
    );
    let found = store.project_in(&user, chat).await.unwrap().unwrap();
    assert_eq!(found.id, made.id);
    assert!(store.project_in(&bob, chat).await.unwrap().is_none());
    assert!(store.ensure_project_agent(&bob, made.id).await.is_err());
    let title: Option<String> = sqlx::query_scalar("SELECT title FROM chats WHERE id=$1")
        .bind(chat)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(title.as_deref(), Some("acme/widget"));
}

#[tokio::test]
async fn slugs_stay_unique() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();

    let made = store
        .create_tasks(project.id, &[new_task("Fix it"), new_task("Fix it")])
        .await
        .unwrap();
    let slugs: Vec<&str> = made.iter().map(|task| task.slug.as_str()).collect();
    assert_eq!(slugs, ["fix-it", "fix-it-2"]);
    let third = store
        .create_tasks(project.id, &[new_task("Fix it!")])
        .await
        .unwrap();
    assert_eq!(third[0].slug.as_str(), "fix-it-3");
    let found = store
        .task_by_slug(project.id, "fix-it-2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.id, made[1].id);
    assert_eq!(found.state, TaskState::Planned);
}

#[tokio::test]
async fn tasks_wait_for_earlier_ones_of_the_same_call() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();

    let second = NewTask {
        depends_on: Some(After::Earlier(0)),
        ..new_task("Render")
    };
    let made = store
        .create_tasks(project.id, &[new_task("Build"), second])
        .await
        .unwrap();
    assert_eq!(made[1].depends_on, Some(made[0].id));
    let later = NewTask {
        depends_on: Some(After::Earlier(1)),
        ..new_task("Ship")
    };
    assert!(store.create_tasks(project.id, &[later]).await.is_err());
    assert_eq!(store.tasks(project.id).await.unwrap().len(), 2);
    store.set_task_after(made[1].id, None).await.unwrap();
    let render = store.task_by_slug(project.id, "render").await.unwrap();
    assert_eq!(render.unwrap().depends_on, None);
}
