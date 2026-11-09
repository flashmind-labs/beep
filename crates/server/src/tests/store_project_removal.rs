use super::*;
use crate::{
    projects::threads::ThreadKind,
    store::{CheckoutRow, NewTask, NewThreadChat, TestDb},
};
use beep_projects::imports::records::{NewImport, SyncRow};
use beep_projects::model::{IssueHost, MachinePath, RepoName};

use iroh::{EndpointId, SecretKey};

/// One project of a user's, with a row in every table that keeps one.
struct Made {
    project: ProjectId,
    chats: Vec<Uuid>,
    machine: EndpointId,
}

async fn make(store: &Store, user: &UserKey, name: &str) -> Made {
    let machine = SecretKey::generate().public();
    let endpoint = machine.to_string();
    let repo = name.parse::<RepoName>().expect("a repo name");
    let project = store.ensure_project(user, &repo).await.expect("made").id;
    let agent = store
        .ensure_project_agent(user, project)
        .await
        .expect("a chat");
    let thread = store
        .open_thread_chat(
            user,
            project,
            &NewThreadChat {
                repo: name,
                number: 7,
                kind: ThreadKind::Issue,
                title: "A bug",
            },
        )
        .await
        .expect("a thread chat");
    store
        .note_thread_reply(project, name, 7)
        .await
        .expect("a reply noted");
    store
        .add_checkout(
            project,
            &CheckoutRow {
                machine,
                path: "/srv/code".parse::<MachinePath>().expect("a path"),
                home: true,
            },
        )
        .await
        .expect("a checkout");
    // Rows older apps' Not now left behind.
    sqlx::query(
        "INSERT INTO project_dismissed_issues (project_id, host, number) VALUES ($1, $2, 3)",
    )
    .bind(project.0)
    .bind(String::from(&IssueHost::Github))
    .execute(&store.pool)
    .await
    .expect("dismissed");
    let task = store
        .create_tasks(
            project,
            &[NewTask {
                title: "Fix it".into(),
                body: String::new(),
                depends_on: None,
                needs: Vec::new(),
                start: None,
            }],
        )
        .await
        .expect("a task")[0]
        .id;
    sqlx::query(
        "INSERT INTO project_workers (id, task_id, project_id, user_id, machine, agent, folder)
             VALUES ($1, $2, $3, $4, $5, 'claude', '/srv/code/.worktrees/fix-it')",
    )
    .bind(Uuid::new_v4())
    .bind(task.0)
    .bind(project.0)
    .bind(user.as_ref())
    .bind(&endpoint)
    .execute(&store.pool)
    .await
    .expect("a worker");
    store
        .save_import(
            user,
            NewImport {
                project: name,
                endpoint: &endpoint,
                folder: "/srv/code",
                url: "https://github.com/acme/widget",
            },
        )
        .await
        .expect("imported");
    // Pinning drops syncs, so it goes first.
    store.pin_project(user, name, true).await.expect("pinned");
    store
        .set_project_sync(
            user,
            &SyncRow {
                project: name.into(),
                endpoint: endpoint.clone(),
                folder: "/srv/code".into(),
            },
        )
        .await
        .expect("synced");
    store
        .pin_project_to_top(user, name, true)
        .await
        .expect("on top");
    sqlx::query(
        "INSERT INTO terminals (id, user_id, project, endpoint_id, task_id, name, command, cwd)
             VALUES ($1, $2, $3, $4, 4, 'dev', 'npm run dev', '/srv/code')",
    )
    .bind(Uuid::new_v4())
    .bind(user.as_ref())
    .bind(name)
    .bind(&endpoint)
    .execute(&store.pool)
    .await
    .expect("a terminal");
    let review = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO branch_reviews (id, user_id, project, branch, base, endpoint_id, folder,
                 head, merge_base, head_at, changed_lines, files)
             VALUES ($1, $2, $3, 'fix', 'main', $4, '/srv/code', 'abc', 'def', now(), 1, 1)",
    )
    .bind(review)
    .bind(user.as_ref())
    .bind(name)
    .bind(&endpoint)
    .execute(&store.pool)
    .await
    .expect("a review");
    sqlx::query(
        "INSERT INTO branch_review_shards (review_id, shard, area, paths, files, lines,
                 agent, session, state)
             VALUES ($1, 0, 'all', '{}', 1, 1, 'claude', 'review-session', 'reading')",
    )
    .bind(review)
    .execute(&store.pool)
    .await
    .expect("a shard");
    Made {
        project,
        chats: vec![agent, thread.chat],
        machine,
    }
}

/// The rows of `made` left, in every table.
async fn left(store: &Store, user: &UserKey, name: &str, made: &Made) -> i64 {
    let by_project = [
        "projects WHERE id",
        "project_checkouts WHERE project_id",
        "project_agents WHERE project_id",
        "project_tasks WHERE project_id",
        "project_workers WHERE project_id",
        "project_dismissed_issues WHERE project_id",
        "project_thread_chats WHERE project_id",
        "project_thread_replies WHERE project_id",
    ];
    let by_name = [
        "project_imports",
        "project_sync",
        "pinned_projects",
        "projects_on_top",
        "paused_syncs",
        "terminals",
        "branch_reviews",
    ];
    let mut total = 0;

    for table in by_project {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}=$1"))
            .bind(made.project.0)
            .fetch_one(&store.pool)
            .await
            .expect("counted");
        total += n;
    }

    for table in by_name {
        let n: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE user_id=$1 AND project=$2"
        ))
        .bind(user.as_ref())
        .bind(name)
        .fetch_one(&store.pool)
        .await
        .expect("counted");
        total += n;
    }

    let shards: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM branch_review_shards s JOIN branch_reviews r ON r.id = s.review_id
             WHERE r.user_id=$1",
    )
    .bind(user.as_ref())
    .fetch_one(&store.pool)
    .await
    .expect("counted");
    let chats: i64 = sqlx::query_scalar("SELECT count(*) FROM chats WHERE id = ANY($1)")
        .bind(&made.chats)
        .fetch_one(&store.pool)
        .await
        .expect("counted");
    total + shards + chats
}

#[tokio::test]
async fn removing_a_project_leaves_no_row_and_spares_other_users() {
    let db = TestDb::new().await.expect("a test database");
    let store = db.store().await.expect("a store");
    let alice = UserKey::for_test("alice");
    let bob = UserKey::for_test("bob");
    let mine = make(&store, &alice, "acme/widget").await;
    let theirs = make(&store, &bob, "acme/widget").await;
    let before = left(&store, &bob, "acme/widget", &theirs).await;
    assert!(before >= 17, "every table has a row: {before}");
    assert_eq!(left(&store, &alice, "acme/widget", &mine).await, before);

    let removed = store
        .remove_project(&alice, "Acme/Widget")
        .await
        .expect("removed")
        .expect("it was there");
    assert_eq!(removed.projects, [mine.project]);
    assert_eq!(removed.workers.len(), 1);
    assert_eq!(removed.workers[0].machine, mine.machine);
    assert_eq!(removed.terminals.len(), 1);
    assert_eq!(removed.reviewers.len(), 1);
    assert_eq!(removed.reviewers[0].session, "review-session");
    assert!(removed.synced);
    assert_eq!(left(&store, &alice, "acme/widget", &mine).await, 0);
    assert_eq!(left(&store, &bob, "acme/widget", &theirs).await, before);

    assert!(
        store
            .remove_project(&alice, "acme/widget")
            .await
            .expect("read")
            .is_none()
    );
}
