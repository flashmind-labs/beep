use super::{
    super::{
        model::{TaskState, WorkerState},
        schedule::Due,
        turn::CHECK_LABEL,
    },
    *,
};
use crate::{
    account::user::User,
    projects::{
        lifecycle::checks::check,
        model::StartRequest,
        testing::{FakeFleet, FakeShell},
    },
    protocol::node::agent::Status,
    store::{CheckoutRow, NewTask, ProjectRow, TaskRow, TestDb, WorkerRow},
};
use beep_projects::model::IssueHost;
use beep_projects::model::{IssueRef, MachinePath, RepoName, Roles, Visibility};
use chrono::{DateTime, TimeDelta, Utc};
use iroh::EndpointId;
use std::sync::Arc;
use uuid::Uuid;

const PR_URL: &str = "https://github.com/acme/widget/pull/7";

struct Harness {
    _db: TestDb,
    user: Arc<User>,
    project: ProjectRow,
    fleet: FakeFleet,
    shell: Arc<FakeShell>,
    home: EndpointId,
}

/// `acme/widget` on GitHub, checked out at `/srv/widget` on `build-box`.
async fn harness() -> Harness {
    let db = TestDb::new().await.expect("a test database");
    let user = User::for_test(db.store().await.expect("a store"), "alice");
    let mut fleet = FakeFleet::default();
    let (home, shell) = fleet.add(1, "build-box", &[], "/srv");
    let project = user
        .ensure_project(&RepoName::parse("acme/widget").expect("a repo"))
        .await
        .expect("the project");
    user.add_checkout(
        project.id,
        &CheckoutRow {
            machine: home,
            path: MachinePath::parse("/srv/widget").expect("a path"),
            home: true,
        },
    )
    .await
    .expect("the checkout");
    let roles = Roles {
        issues: Some(IssueHost::Github),
        prs: Some(IssueHost::Github),
    };
    user.set_project_setup(project.id, &roles, Visibility::Private, "main")
        .await
        .expect("the setup");
    let project = user
        .project(project.id)
        .await
        .expect("the project")
        .expect("it exists");
    Harness {
        _db: db,
        user,
        project,
        fleet,
        shell,
        home,
    }
}

fn new_task(title: &str) -> NewTask {
    NewTask {
        title: title.to_owned(),
        body: "Make it spin.".to_owned(),
        depends_on: None,
        needs: Vec::new(),
        start: None,
    }
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

impl Harness {
    /// A task waiting for a free slot.
    async fn queued(&self, title: &str) -> TaskRow {
        let task = self
            .user
            .create_tasks(self.project.id, &[new_task(title)])
            .await
            .expect("a task")
            .remove(0);
        self.user
            .set_task_start(
                task.id,
                Some(&StartRequest {
                    base: None,
                    machine: None,
                    agent: None,
                    persona: None,
                    inputs: Vec::new(),
                }),
            )
            .await
            .expect("queued");
        task
    }

    /// A task in `state` with issue #12 (and its PR in review), worker's last turn at `last_turn`.
    async fn working(&self, state: TaskState, last_turn: DateTime<Utc>) -> (TaskRow, WorkerRow) {
        let user = &self.user;
        let task = user
            .create_tasks(self.project.id, &[new_task("Spin it")])
            .await
            .expect("the task")
            .remove(0);
        user.set_task_issue(
            task.id,
            &IssueHost::Github,
            &IssueRef {
                number: 12,
                url: "https://github.com/acme/widget/issues/12".into(),
            },
        )
        .await
        .expect("the issue");
        let checkout = MachinePath::parse("/srv/widget").expect("a path");
        // Nothing starts workers now; a row as an older server left it.
        let pool = self._db.raw_pool().await.expect("a pool");
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO project_workers
                 (id, task_id, project_id, user_id, machine, agent, folder, session, state,
                  turns_seen, last_output, last_turn_at)
             VALUES ($1, $2, $3, $4, $5, 'claude', $6, 's1', $7, 1, 'Done.', $8)
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(task.id.0)
        .bind(self.project.id.0)
        .bind(user.id())
        .bind(self.home.to_string())
        .bind(task.slug.worktree(&checkout).to_string())
        .bind(<&str>::from(WorkerState::Waiting))
        .bind(last_turn)
        .fetch_one(&pool)
        .await
        .expect("the worker");
        user.set_task_state(task.id, state)
            .await
            .expect("the state");
        if state == TaskState::Review {
            sqlx::query("UPDATE project_tasks SET pr_url=$2 WHERE id=$1")
                .bind(task.id.0)
                .bind(PR_URL)
                .execute(&pool)
                .await
                .expect("the PR");
        }
        let worker = user
            .project_worker(WorkerId(id))
            .await
            .expect("the worker")
            .expect("it exists");
        (self.task(&task).await, self.worker(&worker).await)
    }

    async fn task(&self, task: &TaskRow) -> TaskRow {
        self.user
            .project_task(task.id)
            .await
            .expect("the task")
            .expect("it exists")
    }

    async fn worker(&self, worker: &WorkerRow) -> WorkerRow {
        self.user
            .project_worker(worker.id)
            .await
            .expect("the worker")
            .expect("it exists")
    }

    /// The PR view answers with `json`.
    fn pr(&self, json: &str) {
        self.shell.answer("gh pr view 7", 0, json);
    }
}

fn minutes_ago(minutes: i64) -> DateTime<Utc> {
    now() - TimeDelta::minutes(minutes)
}

#[tokio::test]
async fn idle_close_keeps_review_tasks() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Review, minutes_ago(31)).await;
    assert_eq!(worker.state, WorkerState::Waiting);
    idle_close(&h.fleet, &h.user, worker.id, now())
        .await
        .expect("it closes");
    assert_eq!(h.worker(&worker).await.state, WorkerState::Ended);
    assert_eq!(h.task(&task).await.state, TaskState::Review);
    assert_eq!(h.fleet.closes(), vec![(h.home, "s1".to_owned())]);
    let posts = h.fleet.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        posts[0].1.contains("Its task stays as it was."),
        "{posts:?}"
    );
    // A review task's issue keeps its labels.
    assert!(h.shell.ran_with("issue edit").is_empty());
}

#[tokio::test]
async fn idle_close_blocks_running_tasks() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Running, minutes_ago(31)).await;
    idle_close(&h.fleet, &h.user, worker.id, now())
        .await
        .expect("it closes");
    assert_eq!(h.worker(&worker).await.state, WorkerState::Ended);
    assert_eq!(h.task(&task).await.state, TaskState::Blocked);
    let relabeled = h.shell.ran_with("issue edit 12");
    assert_eq!(relabeled.len(), 1, "{:?}", h.shell.ran());
    assert!(relabeled[0].contains("--add-label beep:blocked --remove-label beep:running"));
}

#[tokio::test]
async fn a_steered_worker_is_not_idle() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Running, minutes_ago(31)).await;
    // Prompted outside project_worker: the store still says waiting.
    *h.fleet.live.lock().expect("the fake") = Some(Status::Working);
    idle_close(&h.fleet, &h.user, worker.id, now())
        .await
        .expect("it looks");
    assert_eq!(h.worker(&worker).await.state, WorkerState::Waiting);
    assert_eq!(h.task(&task).await.state, TaskState::Running);
    assert!(h.fleet.closes().is_empty());
    let due = h
        .fleet
        .projects
        .armed_at(&Due::Idle(worker.id))
        .expect("armed again");
    assert!(due > now() + TimeDelta::minutes(29), "{due}");
    // Idle for real, it closes.
    *h.fleet.live.lock().expect("the fake") = Some(Status::Idle);
    idle_close(&h.fleet, &h.user, worker.id, now())
        .await
        .expect("it closes");
    assert_eq!(h.worker(&worker).await.state, WorkerState::Ended);
}

#[tokio::test]
async fn stopping_a_worker_blocks_its_task_and_starts_a_queued_one() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Running, minutes_ago(5)).await;
    let queued = h.queued("Next one").await;
    crate::projects::worker::stop_worker_on(&h.fleet, &h.user, &worker)
        .await
        .expect("it stops");
    assert_eq!(h.fleet.starts(), vec![queued.slug]);
    assert_eq!(h.task(&task).await.state, TaskState::Blocked);
    let relabeled = h.shell.ran_with("issue edit 12");
    assert_eq!(relabeled.len(), 1, "{:?}", h.shell.ran());
    assert!(relabeled[0].contains("--add-label beep:blocked --remove-label beep:running"));
}

#[tokio::test]
async fn merged_pr_finishes_the_task() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Review, minutes_ago(5)).await;
    let queued = h.queued("Next one").await;
    h.pr(
        r#"{"number":7,"url":"https://github.com/acme/widget/pull/7","state":"MERGED",
        "mergedAt":"2026-11-01T10:00:00Z","reviewDecision":"APPROVED","statusCheckRollup":[],
        "title":"Spin it","body":"Closes #12"}"#,
    );
    check(&h.fleet, &h.user, h.project.id, now())
        .await
        .expect("it checks");
    assert_eq!(h.task(&task).await.state, TaskState::Done);
    assert_eq!(h.worker(&worker).await.state, WorkerState::Ended);
    let closed = h.shell.ran_with("gh issue close 12");
    assert_eq!(closed.len(), 1, "{:?}", h.shell.ran());
    assert!(
        closed[0].contains(&format!("'Done in {PR_URL}'")),
        "{closed:?}"
    );
    let removed = h.shell.ran_with("worktree remove");
    assert_eq!(removed.len(), 1, "{:?}", h.shell.ran());
    assert!(
        removed[0].contains("/srv/widget/.worktrees/spin-it"),
        "{removed:?}"
    );
    assert_eq!(h.fleet.starts(), vec![queued.slug.clone()]);
    let posts = h.fleet.posts();
    assert!(
        posts
            .iter()
            .any(|(label, text)| label == CHECK_LABEL && text.contains("is merged")),
        "{posts:?}"
    );
    // Nothing is open to checks now.
    assert_eq!(h.fleet.projects.armed_at(&Due::Check(h.project.id)), None);
}

#[tokio::test]
async fn a_failed_check_still_checks_again() {
    let h = harness().await;
    h.working(TaskState::Review, minutes_ago(5)).await;
    h.working(TaskState::Review, minutes_ago(5)).await;
    h.pr(
        r#"{"number":7,"url":"https://github.com/acme/widget/pull/7","state":"MERGED",
        "mergedAt":"2026-11-01T10:00:00Z","reviewDecision":"APPROVED","statusCheckRollup":[],
        "title":"Spin it","body":"Closes #12"}"#,
    );
    h.shell.fail_on("worktree remove");
    assert!(check(&h.fleet, &h.user, h.project.id, now()).await.is_err());
    assert!(
        h.fleet
            .projects
            .armed_at(&Due::Check(h.project.id))
            .is_some()
    );
}

#[tokio::test]
async fn unchanged_checks_post_nothing() {
    let h = harness().await;
    let (task, _) = h.working(TaskState::Review, minutes_ago(5)).await;
    h.pr(
        r#"{"number":7,"url":"https://github.com/acme/widget/pull/7","state":"OPEN",
        "mergedAt":null,"reviewDecision":"REVIEW_REQUIRED",
        "statusCheckRollup":[{"conclusion":"SUCCESS"}],"title":"Spin it","body":""}"#,
    );
    check(&h.fleet, &h.user, h.project.id, now())
        .await
        .expect("it checks");
    assert_eq!(h.fleet.posts().len(), 1, "{:?}", h.fleet.posts());
    check(&h.fleet, &h.user, h.project.id, now())
        .await
        .expect("it checks again");
    assert_eq!(h.fleet.posts().len(), 1, "{:?}", h.fleet.posts());
    assert_eq!(h.task(&task).await.state, TaskState::Review);
    // Review is open to checks, so the timer runs on.
    let due = h
        .fleet
        .projects
        .armed_at(&Due::Check(h.project.id))
        .expect("armed");
    assert!(due > now() + TimeDelta::minutes(9), "{due}");
}
