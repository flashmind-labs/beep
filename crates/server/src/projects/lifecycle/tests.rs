use super::*;
use crate::{
    projects::{
        model::{IssueRef, MachinePath, RepoName, Roles, StartRequest, Visibility, WorkerAgent},
        testing::{FakeAttach, FakeFleet, FakeShell},
    },
    store::{CheckoutRow, NewTask, NewWorker, TestDb},
};
use iroh::EndpointId;

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
    /// A task in `state`, with issue #12, its PR when in review, and a
    /// worker that finished a turn at `last_turn`.
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
        let worker = user
            .start_worker(&NewWorker {
                task: task.id,
                machine: self.home,
                agent: WorkerAgent::Claude,
                folder: task.slug.worktree(&checkout),
                persona: None,
                persona_commit: None,
            })
            .await
            .expect("the store answers")
            .expect("the worker starts");
        user.set_worker_session(worker.id, "s1")
            .await
            .expect("the session");
        user.set_worker_state(worker.id, WorkerState::Running)
            .await
            .expect("running");
        assert!(
            user.worker_turn(worker.id, 1, "Done.", last_turn)
                .await
                .expect("a turn")
        );
        user.set_task_state(task.id, state)
            .await
            .expect("the state");
        if state == TaskState::Review {
            user.set_task_pr(task.id, PR_URL).await.expect("the PR");
        }
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
async fn asking_workers_are_not_idle() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Running, minutes_ago(31)).await;
    h.user
        .set_worker_asking(worker.id, true)
        .await
        .expect("asking");
    idle_close(&h.fleet, &h.user, worker.id, now())
        .await
        .expect("it looks");
    assert_eq!(h.worker(&worker).await.state, WorkerState::Waiting);
    assert_eq!(h.task(&task).await.state, TaskState::Running);
    assert!(h.fleet.closes().is_empty());
    // Not idle long enough: armed again for when it will be.
    let fresh = h.working(TaskState::Running, minutes_ago(5)).await.1;
    idle_close(&h.fleet, &h.user, fresh.id, now())
        .await
        .expect("it looks");
    assert_eq!(h.worker(&fresh).await.state, WorkerState::Waiting);
    let due = h
        .fleet
        .projects
        .armed_at(&Due::Idle(fresh.id))
        .expect("armed again");
    assert!(due > now() + TimeDelta::minutes(20), "{due}");
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
async fn merged_pr_finishes_the_task() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Review, minutes_ago(5)).await;
    let queued = h
        .user
        .create_tasks(h.project.id, &[new_task("Next one")])
        .await
        .expect("a task")
        .remove(0);
    h.user
        .set_task_start(
            queued.id,
            Some(&StartRequest {
                machine: None,
                agent: None,
                persona: None,
                inputs: Vec::new(),
            }),
        )
        .await
        .expect("queued");
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

#[tokio::test]
async fn a_pr_moves_a_running_task_to_review() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Running, minutes_ago(5)).await;
    h.shell.answer(
        "gh pr list",
        0,
        r#"[{"number":7,"url":"https://github.com/acme/widget/pull/7","state":"OPEN",
        "mergedAt":null,"reviewDecision":"","statusCheckRollup":[],
        "title":"Spin it","body":"Built on build-box"}]"#,
    );
    let done = TurnDone {
        turn: 2,
        last_message: "Opened the pull request.",
        report: true,
    };
    turn_done(&h.fleet, &h.user, worker.id, &done, now())
        .await
        .expect("it reports");
    let task = h.task(&task).await;
    assert_eq!(task.state, TaskState::Review);
    assert_eq!(task.pr_url.as_deref(), Some(PR_URL));
    let posts = h.fleet.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!(posts[0].0, WORKER_LABEL);
    assert!(
        posts[0].1.contains(&format!("Pull request: {PR_URL}")),
        "{posts:?}"
    );
    assert!(
        posts[0].1.contains("names private details: build-box"),
        "{posts:?}"
    );
    assert!(
        h.fleet
            .projects
            .armed_at(&Due::Check(h.project.id))
            .is_some()
    );
    // The same turn again is not news.
    turn_done(&h.fleet, &h.user, worker.id, &done, now())
        .await
        .expect("it reports");
    assert_eq!(h.fleet.posts().len(), 1);
}

#[tokio::test]
async fn questions_inside_the_worktree_are_answered() {
    use crate::protocol::node::agent::AnswerOption;
    let h = harness().await;
    let (_, worker) = h.working(TaskState::Running, minutes_ago(5)).await;
    let question = |id, text: &str, detail: &str| Question {
        id,
        text: text.to_owned(),
        detail: Some(detail.to_owned()),
        options: vec![
            AnswerOption {
                id: "allow".into(),
                name: "Allow".into(),
                kind: OptionKind::AllowOnce,
            },
            AnswerOption {
                id: "reject".into(),
                name: "Reject".into(),
                kind: OptionKind::RejectOnce,
            },
        ],
    };
    let inside = question(
        1,
        "Edit notes.txt",
        "/srv/widget/.worktrees/spin-it/notes.txt",
    );
    asked(&h.fleet, &h.user, worker.id, inside, true)
        .await
        .expect("it judges");
    assert_eq!(
        h.fleet.sends(),
        vec![AgentRequest::Answer {
            question: 1,
            reply: "allow".into()
        }]
    );
    assert!(h.fleet.posts().is_empty());
    let force = question(2, "Run git push", "git push --force origin beep/spin-it");
    asked(&h.fleet, &h.user, worker.id, force, true)
        .await
        .expect("it judges");
    let sends = h.fleet.sends();
    assert_eq!(
        sends[1],
        AgentRequest::Answer {
            question: 2,
            reply: "reject".into()
        }
    );
    assert!(matches!(&sends[2], AgentRequest::Prompt { text } if text.contains("Force pushes")));
    let outside = question(3, "Run rm", "rm -rf /srv/other");
    asked(&h.fleet, &h.user, worker.id, outside.clone(), true)
        .await
        .expect("it judges");
    assert_eq!(h.fleet.questions(), vec![outside.clone()]);
    assert!(h.worker(&worker).await.asking);
    // Seen again on a re-attach, it is not asked twice.
    asked(&h.fleet, &h.user, worker.id, outside, true)
        .await
        .expect("it judges");
    assert_eq!(h.fleet.questions().len(), 1);
}

#[tokio::test]
async fn a_failed_reattach_leaves_the_worker() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Running, minutes_ago(5)).await;
    reattach(&h.fleet, &h.user, &worker)
        .await
        .expect("it tries");
    assert_eq!(h.worker(&worker).await.state, WorkerState::Waiting);
    assert_eq!(h.task(&task).await.state, TaskState::Running);
    assert!(h.fleet.posts().is_empty(), "{:?}", h.fleet.posts());
    assert_eq!(h.fleet.projects.worker_of(&h.home, "s1"), Some(worker.id));
    // Refused: the machine no longer has the session.
    *h.fleet.attach.lock().expect("the fake") = FakeAttach::Refused;
    reattach(&h.fleet, &h.user, &worker)
        .await
        .expect("it tries");
    assert_eq!(h.worker(&worker).await.state, WorkerState::Failed);
    assert_eq!(h.task(&task).await.state, TaskState::Blocked);
    let posts = h.fleet.posts();
    assert!(
        posts
            .iter()
            .any(|(label, text)| label == REPLAN_LABEL && text.contains("is gone")),
        "{posts:?}"
    );
}

#[tokio::test]
async fn a_late_turn_report_changes_nothing() {
    let h = harness().await;
    let (_, worker) = h.working(TaskState::Running, minutes_ago(5)).await;
    let user = &h.user;
    assert!(
        user.worker_turn(worker.id, 3, "Third.", now())
            .await
            .expect("a turn")
    );
    user.set_worker_asking(worker.id, true)
        .await
        .expect("asking");
    // Turn 2's report, handled after turn 3's and the question.
    let noted = user
        .worker_turn(worker.id, 2, "Second.", now())
        .await
        .expect("a turn");
    assert!(!noted);
    let row = h.worker(&worker).await;
    assert_eq!(row.turns_seen, 3);
    assert!(row.asking);
    assert_eq!(row.last_output.as_deref(), Some("Third."));
}

#[tokio::test]
async fn a_worker_ending_while_its_turn_is_handled_keeps_the_task_blocked() {
    let h = harness().await;
    let (task, worker) = h.working(TaskState::Running, minutes_ago(5)).await;
    h.shell.answer(
        "gh pr list",
        0,
        r#"[{"number":7,"url":"https://github.com/acme/widget/pull/7","state":"OPEN",
        "mergedAt":null,"reviewDecision":"","statusCheckRollup":[],
        "title":"Spin it","body":""}]"#,
    );
    let release = h.shell.block_on("gh pr list");
    let done = TurnDone {
        turn: 2,
        last_message: "Opened the pull request.",
        report: true,
    };
    let report = turn_done(&h.fleet, &h.user, worker.id, &done, now());
    // The worker ends while the report looks for its pull request.
    let end = async {
        h.shell.until_blocked().await;
        ended(&h.fleet, &h.user, worker.id, "it crashed")
            .await
            .expect("it ends");
        release.notify_one();
    };
    let (reported, ()) = tokio::join!(report, end);
    reported.expect("it reports");
    let task = h.task(&task).await;
    assert_eq!(task.state, TaskState::Blocked);
    assert_eq!(task.pr_url, None);
    assert_eq!(h.fleet.projects.armed_at(&Due::Check(h.project.id)), None);
}
