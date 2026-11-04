use super::{super::Store, *};
use crate::{
    account::user::UserKey,
    projects::model::{
        MachinePath, ProjectId, RepoName, TaskId, TaskState, Visibility, WorkerAgent, WorkerState,
    },
    store::TestDb,
};
use anyhow::Result;
use iroh::{EndpointId, SecretKey};

fn machine() -> EndpointId {
    SecretKey::generate().public()
}

fn repo(name: &str) -> RepoName {
    RepoName::parse(name).expect("a valid repo name")
}

fn path(text: &str) -> MachinePath {
    MachinePath::parse(text).expect("a valid path")
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

async fn tasks_in(store: &Store, project: ProjectId, count: usize) -> Vec<TaskId> {
    let titles: Vec<NewTask> = (0..count).map(|n| new_task(&format!("Task {n}"))).collect();
    store
        .create_tasks(project, &titles)
        .await
        .expect("tasks are made")
        .into_iter()
        .map(|task| task.id)
        .collect()
}

fn worker_for(task: TaskId, machine: EndpointId, folder: &str) -> NewWorker {
    NewWorker {
        task,
        machine,
        agent: WorkerAgent::Claude,
        folder: path(folder),
        persona: None,
        persona_commit: None,
    }
}

async fn start(
    store: &Store,
    user: &UserKey,
    worker: &NewWorker,
) -> Result<WorkerRow, StartRefused> {
    store
        .start_worker(user, worker)
        .await
        .expect("the store answers")
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

#[tokio::test]
async fn a_refused_start_leaves_no_worker() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let tasks = tasks_in(&store, project.id, 4).await;
    for (n, task) in tasks.iter().take(3).enumerate() {
        start(
            &store,
            &user,
            &worker_for(*task, machine(), &format!("/srv/{n}")),
        )
        .await
        .unwrap();
    }
    let refused = start(&store, &user, &worker_for(tasks[3], machine(), "/srv/3")).await;
    assert!(matches!(refused, Err(StartRefused::ProjectFull)));
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_workers")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(rows, 3);
}

#[tokio::test]
async fn an_ended_worker_stays_ended() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let tasks = tasks_in(&store, project.id, 1).await;
    let worker = start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
        .await
        .unwrap();
    assert!(
        store
            .set_worker_state(worker.id, WorkerState::Running)
            .await
            .unwrap()
    );
    assert!(
        store
            .set_worker_state(worker.id, WorkerState::Ended)
            .await
            .unwrap()
    );
    let ended_at = store
        .worker(&user, worker.id)
        .await
        .unwrap()
        .unwrap()
        .ended_at;
    assert!(ended_at.is_some());
    for live in WorkerState::LIVE {
        assert!(!store.set_worker_state(worker.id, live).await.unwrap());
    }
    let row = store.worker(&user, worker.id).await.unwrap().unwrap();
    assert_eq!(row.state, WorkerState::Ended);
    assert_eq!(row.ended_at, ended_at);
    // Ending it another way still moves it, keeping when it ended.
    assert!(
        store
            .set_worker_state(worker.id, WorkerState::Failed)
            .await
            .unwrap()
    );
    let row = store.worker(&user, worker.id).await.unwrap().unwrap();
    assert_eq!(row.state, WorkerState::Failed);
    assert_eq!(row.ended_at, ended_at);
}

#[tokio::test]
async fn caps_refuse_past_two_per_machine() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let widget = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let gadget = store
        .ensure_project(&user, &repo("acme/gadget"))
        .await
        .unwrap();
    let widget_tasks = tasks_in(&store, widget.id, 2).await;
    let gadget_tasks = tasks_in(&store, gadget.id, 1).await;
    let build_box = machine();
    start(
        &store,
        &user,
        &worker_for(widget_tasks[0], build_box, "/srv/a"),
    )
    .await
    .unwrap();
    start(
        &store,
        &user,
        &worker_for(widget_tasks[1], build_box, "/srv/b"),
    )
    .await
    .unwrap();
    let refused = start(
        &store,
        &user,
        &worker_for(gadget_tasks[0], build_box, "/srv/c"),
    )
    .await;
    assert!(matches!(refused, Err(StartRefused::MachineFull)));
    start(
        &store,
        &user,
        &worker_for(gadget_tasks[0], machine(), "/srv/c"),
    )
    .await
    .unwrap();
    assert_eq!(
        store
            .live_workers_on(&user, &build_box)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn caps_refuse_past_six_per_user() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let mut tasks = Vec::new();
    for name in ["acme/one", "acme/two", "acme/three"] {
        let project = store.ensure_project(&user, &repo(name)).await.unwrap();
        tasks.extend(tasks_in(&store, project.id, 2).await);
    }
    let last = store
        .ensure_project(&user, &repo("acme/four"))
        .await
        .unwrap();
    tasks.extend(tasks_in(&store, last.id, 1).await);
    for (n, task) in tasks.iter().take(6).enumerate() {
        start(
            &store,
            &user,
            &worker_for(*task, machine(), &format!("/srv/{n}")),
        )
        .await
        .unwrap();
    }
    let refused = start(&store, &user, &worker_for(tasks[6], machine(), "/srv/6")).await;
    assert!(matches!(refused, Err(StartRefused::UserFull)));
    assert_eq!(store.live_workers(&user).await.unwrap().len(), 6);
    // Another user's slots are their own.
    let bob = UserKey::for_test("bob");
    let theirs = store.ensure_project(&bob, &repo("acme/one")).await.unwrap();
    let bob_task = tasks_in(&store, theirs.id, 1).await;
    start(&store, &bob, &worker_for(bob_task[0], machine(), "/srv/0"))
        .await
        .unwrap();
}

#[tokio::test]
async fn one_folder_one_writer() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let tasks = tasks_in(&store, project.id, 2).await;
    let build_box = machine();
    let first = start(
        &store,
        &user,
        &worker_for(tasks[0], build_box, "/srv/project"),
    )
    .await
    .unwrap();
    let refused = start(
        &store,
        &user,
        &worker_for(tasks[1], build_box, "/srv/project"),
    )
    .await;
    assert!(matches!(refused, Err(StartRefused::FolderTaken)));
    // The same folder on another machine is free.
    start(
        &store,
        &user,
        &worker_for(tasks[1], machine(), "/srv/project"),
    )
    .await
    .unwrap();
    // And on the same machine once the first worker failed.
    store
        .set_worker_state(first.id, WorkerState::Failed)
        .await
        .unwrap();
    let row = store.worker(&user, first.id).await.unwrap().unwrap();
    assert!(row.ended_at.is_some());
    let third = tasks_in(&store, project.id, 1).await;
    start(
        &store,
        &user,
        &worker_for(third[0], build_box, "/srv/project"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn one_worker_per_task() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let tasks = tasks_in(&store, project.id, 1).await;
    start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
        .await
        .unwrap();
    let refused = start(&store, &user, &worker_for(tasks[0], machine(), "/srv/b")).await;
    assert!(matches!(refused, Err(StartRefused::TaskHasWorker)));
}

#[tokio::test]
async fn a_task_of_another_user_does_not_start() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();

    let tasks = tasks_in(&store, project.id, 1).await;
    let bob = UserKey::for_test("bob");
    assert!(
        store
            .start_worker(&bob, &worker_for(tasks[0], machine(), "/srv/a"))
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_starts_take_one_slot() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let tasks = tasks_in(&store, project.id, 4).await;
    start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
        .await
        .unwrap();
    start(&store, &user, &worker_for(tasks[1], machine(), "/srv/b"))
        .await
        .unwrap();
    // Open two connections first, so both starts get one at once and
    // their transactions overlap instead of one waiting on a connect.
    // The test runs multi-threaded so the pool can take them back in
    // between; without the advisory lock both starts then win.
    let (one, two) = tokio::join!(store.pool().acquire(), store.pool().acquire());
    drop((one.unwrap(), two.unwrap()));
    // Both race for the last slot in the project, on separate connections.
    let first = worker_for(tasks[2], machine(), "/srv/c");
    let second = worker_for(tasks[3], machine(), "/srv/d");
    let (one, two) = tokio::join!(
        store.start_worker(&user, &first),
        store.start_worker(&user, &second)
    );
    let results = [one.unwrap(), two.unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(StartRefused::ProjectFull)))
            .count(),
        1
    );
    assert_eq!(store.open_counts(project.id).await.unwrap().workers, 3);
}

#[tokio::test]
async fn starting_workers_fail_on_startup() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let project = store
        .ensure_project(&user, &repo("acme/widget"))
        .await
        .unwrap();
    let tasks = tasks_in(&store, project.id, 2).await;
    let starting = start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
        .await
        .unwrap();
    let running = start(&store, &user, &worker_for(tasks[1], machine(), "/srv/b"))
        .await
        .unwrap();
    store
        .set_worker_state(running.id, WorkerState::Running)
        .await
        .unwrap();
    let failed = store.fail_starting_workers().await.unwrap();
    assert_eq!(
        failed.iter().map(|w| w.id).collect::<Vec<_>>(),
        vec![starting.id]
    );
    assert_eq!(failed[0].state, WorkerState::Failed);
    assert!(failed[0].ended_at.is_some());
    let row = store.worker(&user, running.id).await.unwrap().unwrap();
    assert_eq!(row.state, WorkerState::Running);
    assert_eq!(store.live_workers(&user).await.unwrap().len(), 1);
}
