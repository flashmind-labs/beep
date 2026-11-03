use super::*;
use crate::{
    api::work::pending_approvals,
    projects::{
        model::{MachinePath, WorkerAgent, WorkerState},
        testing::machine_id,
    },
    store::{CreateRun, FinishedTurn, NewTask, NewWorker, RunStatus, Step, StepBody, TurnRecord},
    user::UserKey,
    watchers::testing::{Harness, Scripted},
};
use flashmind::core::{Conversation, ConversationEntry};

async fn harness() -> Harness {
    Harness::new(Scripted::default()).await
}

fn new_task(title: &str) -> NewTask {
    NewTask {
        title: title.to_owned(),
        body: "Make the widget spin.".to_owned(),
        depends_on: None,
        needs: Vec::new(),
        start: None,
    }
}

/// Alice's project `acme/widget` with one planned task.
async fn one_task(h: &Harness) -> (ProjectAgent, TaskRow) {
    let agent = project_agent(&h.alice, "acme/widget").await.unwrap();
    let mut tasks = h
        .alice
        .create_tasks(ProjectId(agent.project_id), &[new_task("Spin it")])
        .await
        .unwrap();
    (agent, tasks.remove(0))
}

fn is_not_found<T: std::fmt::Debug>(reply: Reply<T>) -> bool {
    matches!(reply, Err(ApiError::NotFound {}))
}

fn is_bad_request<T: std::fmt::Debug>(reply: Reply<T>) -> bool {
    matches!(reply, Err(ApiError::BadRequest { .. }))
}

#[tokio::test]
async fn get_project_agent_makes_one_chat() {
    let h = harness().await;
    let first = project_agent(&h.alice, "acme/widget").await.unwrap();
    let again = project_agent(&h.alice, "acme/widget").await.unwrap();
    assert_eq!(first, again);
    assert_eq!(first.name, "acme/widget");
    let listed = projects(&h.alice).await.unwrap();
    assert_eq!(listed, vec![first]);
    assert_eq!(listed[0].open_tasks, 0);
    assert_eq!(listed[0].open_workers, 0);
}

#[tokio::test]
async fn get_project_agent_refuses_bad_names() {
    let h = harness().await;
    for name in ["acme", "acme/widget/more", "", "acme/.."] {
        assert!(
            is_bad_request(project_agent(&h.alice, name).await),
            "{name}"
        );
    }
    assert!(projects(&h.alice).await.unwrap().is_empty());
}

#[tokio::test]
async fn every_user_gets_projects_of_their_own() {
    let h = harness().await;
    let alices = project_agent(&h.alice, "acme/widget").await.unwrap();
    let bobs = project_agent(&h.bob, "acme/widget").await.unwrap();
    assert_ne!(alices.project_id, bobs.project_id);
    assert_ne!(alices.chat_id, bobs.chat_id);
    assert_eq!(projects(&h.bob).await.unwrap(), vec![bobs]);
    assert_eq!(projects(&h.alice).await.unwrap(), vec![alices]);
}

#[tokio::test]
async fn other_users_tasks_are_not_found() {
    let h = harness().await;
    let (agent, task) = one_task(&h).await;
    let project = ProjectId(agent.project_id);
    assert!(is_not_found(project_tasks(&h.bob, project).await));
    assert!(is_not_found(project_workers(&h.bob, project).await));
    let dropped = update_task(&h.app, &h.bob, task.id, TaskState::Dropped).await;
    assert!(is_not_found(dropped));
    assert_eq!(
        h.alice.project_task(task.id).await.unwrap().unwrap().state,
        TaskState::Planned
    );
    assert!(is_not_found(
        project_tasks(&h.alice, ProjectId(Uuid::new_v4())).await
    ));
    let listed = project_tasks(&h.alice, project).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].slug, task.slug.as_str());
    assert_eq!(listed[0].state, ProjectTaskState::Planned {});
}

#[tokio::test]
async fn other_users_cannot_list_track_or_dismiss_issues() {
    let h = harness().await;
    let agent = project_agent(&h.alice, "acme/widget").await.unwrap();
    let project = ProjectId(agent.project_id);
    assert!(is_not_found(
        super::super::project_watch::copied_issues(&h.app, &h.bob, project).await
    ));
    let start = IssueStart {
        agent: "claude".to_owned(),
        node_id: None,
    };
    let tracked = track_issue(&h.app, &h.bob, project, 12, Some(&start)).await;
    assert!(is_not_found(tracked));
    assert!(is_not_found(
        track_issue(&h.app, &h.bob, project, 12, None).await
    ));
    assert!(is_not_found(dismiss_issue(&h.bob, project, 12).await));
    assert!(h.alice.project_tasks(project).await.unwrap().is_empty());
    // Alice's own project without a host lists nothing.
    assert!(
        super::super::project_watch::copied_issues(&h.app, &h.alice, project)
            .await
            .unwrap()
            .is_empty()
    );
    // Nor dismisses one: there is no host to hide it on.
    assert!(is_bad_request(dismiss_issue(&h.alice, project, 12).await));
}

#[tokio::test]
async fn track_project_issue_checks_the_start() {
    let h = harness().await;
    let agent = project_agent(&h.alice, "acme/widget").await.unwrap();
    let project = ProjectId(agent.project_id);
    let bad_agent = IssueStart {
        agent: "vim".to_owned(),
        node_id: None,
    };
    let refused = track_issue(&h.app, &h.alice, project, 1, Some(&bad_agent)).await;
    assert!(is_bad_request(refused));
    let bad_machine = IssueStart {
        agent: "codex".to_owned(),
        node_id: Some("not-a-key".to_owned()),
    };
    let refused = track_issue(&h.app, &h.alice, project, 1, Some(&bad_machine)).await;
    assert!(is_bad_request(refused));
    // A project with no issues host cannot track one.
    assert!(is_bad_request(
        track_issue(&h.app, &h.alice, project, 1, None).await
    ));
    assert!(h.alice.project_tasks(project).await.unwrap().is_empty());
}

#[tokio::test]
async fn update_project_task_drops_and_reopens() {
    let h = harness().await;
    let (agent, task) = one_task(&h).await;
    let dropped = update_task(&h.app, &h.alice, task.id, TaskState::Dropped)
        .await
        .unwrap();
    assert_eq!(dropped.state, ProjectTaskState::Dropped {});
    let counted = projects(&h.alice).await.unwrap();
    assert_eq!(counted[0].project_id, agent.project_id);
    assert_eq!(counted[0].open_tasks, 0);
    let reopened = update_task(&h.app, &h.alice, task.id, TaskState::Planned)
        .await
        .unwrap();
    assert_eq!(reopened.state, ProjectTaskState::Planned {});
    for from in [TaskState::Blocked, TaskState::Done] {
        h.alice.set_task_state(task.id, from).await.unwrap();
        let reopened = update_task(&h.app, &h.alice, task.id, TaskState::Planned)
            .await
            .unwrap();
        assert_eq!(reopened.state, ProjectTaskState::Planned {});
    }
}

#[tokio::test]
async fn update_project_task_refuses_running() {
    let h = harness().await;
    let (_, task) = one_task(&h).await;
    let to_running = update_task(&h.app, &h.alice, task.id, TaskState::Running).await;
    assert!(is_bad_request(to_running));
    // A planned task is planned already.
    let replanned = update_task(&h.app, &h.alice, task.id, TaskState::Planned).await;
    assert!(is_bad_request(replanned));
    h.alice
        .set_task_state(task.id, TaskState::Running)
        .await
        .unwrap();
    for to in [TaskState::Planned, TaskState::Review, TaskState::Done] {
        let refused = update_task(&h.app, &h.alice, task.id, to).await;
        assert!(is_bad_request(refused), "{to:?}");
    }
    // Done twice is not dropped either.
    h.alice
        .set_task_state(task.id, TaskState::Done)
        .await
        .unwrap();
    let dropped = update_task(&h.app, &h.alice, task.id, TaskState::Dropped).await;
    assert!(is_bad_request(dropped));
}

#[tokio::test]
async fn stop_project_worker_is_idempotent() {
    let h = harness().await;
    let (agent, task) = one_task(&h).await;
    let worker = h
        .alice
        .start_worker(&NewWorker {
            task: task.id,
            machine: machine_id(1),
            agent: WorkerAgent::Claude,
            folder: MachinePath::parse("/srv/widget/.worktrees/spin-it").unwrap(),
            persona: None,
            persona_commit: None,
        })
        .await
        .unwrap()
        .expect("a free slot");
    h.alice
        .set_task_state(task.id, TaskState::Running)
        .await
        .unwrap();
    assert!(is_not_found(stop(&h.app, &h.bob, worker.id).await));
    stop(&h.app, &h.alice, worker.id).await.unwrap();
    stop(&h.app, &h.alice, worker.id).await.unwrap();
    let workers = project_workers(&h.alice, ProjectId(agent.project_id))
        .await
        .unwrap();
    assert_eq!(workers.len(), 1, "an ended worker shows for a day");
    assert_eq!(workers[0].state, WorkerState::Ended.into());
    assert_eq!(workers[0].machine_name, "", "the machine is not linked");
    assert_eq!(
        h.alice.project_task(task.id).await.unwrap().unwrap().state,
        TaskState::Blocked
    );
    assert!(is_not_found(
        stop(&h.app, &h.alice, WorkerId(Uuid::new_v4())).await
    ));
}

/// Parks a turn of `chat` on a Project app card and returns the run's id.
async fn park(h: &Harness, chat: Uuid, call: &str) -> String {
    let key = UserKey::for_test(h.alice.id());
    let CreateRun::Created(run) = h
        .app
        .store
        .create_run(&key, chat, &format!("req-{call}"), "Build it")
        .await
        .unwrap()
    else {
        panic!("the run was not created");
    };
    let steps = [
        Step {
            sent_at: Utc::now(),
            body: StepBody::User {
                content: "Build it".to_owned(),
                turn: TurnRecord::default(),
            },
        },
        Step {
            sent_at: Utc::now(),
            body: StepBody::Approval {
                query_id: call.to_owned(),
                approval: Approval {
                    app: "Project".to_owned(),
                    action: "run a command".to_owned(),
                    fields: Vec::new(),
                },
            },
        },
    ];
    let mut context = Conversation::new();
    context.add(ConversationEntry::user("Build it"));
    h.app
        .store
        .finish(
            &key,
            &run,
            Some(FinishedTurn {
                context: &context,
                prompt_tokens: Some(0),
                steps: &steps,
                stands_alone: false,
                carried: TurnRecord::default(),
            }),
            RunStatus::Waiting,
        )
        .await
        .unwrap();
    run.id
}

#[tokio::test]
async fn project_cards_reach_needs_you() {
    let h = harness().await;
    let agent = project_agent(&h.alice, "acme/widget").await.unwrap();
    assert!(
        pending_approvals(&h.app, &h.alice)
            .await
            .unwrap()
            .is_empty()
    );
    let run = park(&h, agent.chat_id, "call-1").await;
    let pending = pending_approvals(&h.app, &h.alice).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].session_id, agent.chat_id);
    assert_eq!(pending[0].session_title, "acme/widget");
    assert_eq!(pending[0].pending.run_id, run);
    assert!(!run.is_empty());
    assert_eq!(pending[0].pending.query_id, "call-1");
    assert_eq!(pending[0].pending.approval.app, "Project");
    assert!(pending_approvals(&h.app, &h.bob).await.unwrap().is_empty());

    // Once a later turn runs, as answering the card starts, it is gone.
    let key = UserKey::for_test(h.alice.id());
    let CreateRun::Created(answered) = h
        .app
        .store
        .create_run(&key, agent.chat_id, "req-answer", "allow")
        .await
        .unwrap()
    else {
        panic!("the run was not created");
    };
    assert!(
        pending_approvals(&h.app, &h.alice)
            .await
            .unwrap()
            .is_empty()
    );
    h.app
        .store
        .finish(&key, &answered, None, RunStatus::Completed)
        .await
        .unwrap();
    assert!(
        pending_approvals(&h.app, &h.alice)
            .await
            .unwrap()
            .is_empty()
    );
}
