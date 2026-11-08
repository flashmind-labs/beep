use super::*;
use beep_proto::Approval;
use std::time::{Duration, SystemTime};

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn task(title: &str, state: ProjectTaskState, created: u64) -> ProjectTask {
    ProjectTask {
        id: Uuid::new_v4(),
        slug: title.replace(' ', "-"),
        title: title.into(),
        body: String::new(),
        state,
        depends_on: None,
        issue_number: None,
        issue_url: None,
        branch: None,
        pr_url: None,
        created_at: at(created),
        updated_at: at(created),
    }
}

fn worker(task_id: Uuid, state: ProjectWorkerState, started: u64) -> ProjectWorker {
    ProjectWorker {
        id: Uuid::new_v4(),
        task_id,
        machine_id: "m1".into(),
        machine_name: "build-box".into(),
        agent: "claude".into(),
        session: None,
        folder: "/srv/project/.worktrees/task".into(),
        state,
        last_output: None,
        persona: None,
        started_at: at(started),
        ended_at: None,
    }
}

fn approval(chat: Uuid, query: &str) -> ChatApproval {
    ChatApproval {
        session_id: chat,
        session_title: "acme/widget".into(),
        pending: PendingApproval {
            run_id: "run".into(),
            query_id: query.into(),
            approval: Approval {
                app: "Project".into(),
                action: "Create 3 tasks".into(),
                fields: Vec::new(),
            },
        },
    }
}

fn parts() -> ProjectParts {
    ProjectParts {
        chat_id: Uuid::new_v4(),
        tasks: Vec::new(),
        workers: Vec::new(),
        turns: Vec::new(),
        approvals: Vec::new(),
    }
}

#[test]
fn tasks_order_by_state() {
    let view = project_view(ProjectParts {
        tasks: vec![
            task("dropped", ProjectTaskState::Dropped {}, 1),
            task("done", ProjectTaskState::Done {}, 2),
            task("planned late", ProjectTaskState::Planned {}, 9),
            task("planned early", ProjectTaskState::Planned {}, 3),
            task("blocked", ProjectTaskState::Blocked {}, 4),
            task("review", ProjectTaskState::Review {}, 5),
            task("running", ProjectTaskState::Running {}, 6),
        ],
        ..parts()
    });
    let titles: Vec<&str> = view.tasks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "running",
            "review",
            "blocked",
            "planned early",
            "planned late",
            "done",
            "dropped"
        ]
    );
    assert_eq!(view.tasks[0].state, "running");
}

#[test]
fn workers_live_first_then_newest() {
    let t = Uuid::new_v4();
    let view = project_view(ProjectParts {
        workers: vec![
            worker(t, ProjectWorkerState::Ended {}, 50),
            worker(t, ProjectWorkerState::Running {}, 10),
            worker(t, ProjectWorkerState::Failed {}, 60),
            worker(t, ProjectWorkerState::Waiting {}, 20),
        ],
        ..parts()
    });
    let order: Vec<(u64, bool)> = view
        .workers
        .iter()
        .map(|w| (w.started_at / 1000, w.live))
        .collect();
    assert_eq!(order, [(20, true), (10, true), (60, false), (50, false)]);
}

#[test]
fn cards_only_for_this_chat() {
    let base = parts();
    let chat = base.chat_id;
    let view = project_view(ProjectParts {
        approvals: vec![
            approval(Uuid::new_v4(), "elsewhere"),
            approval(chat, "plan"),
        ],
        ..base
    });
    let queries: Vec<&str> = view.cards.iter().map(|c| c.query_id.as_str()).collect();
    assert_eq!(queries, ["plan"]);
}

#[test]
fn task_rows_link_their_live_worker() {
    let busy = task("busy", ProjectTaskState::Running {}, 1);
    let idle = task("idle", ProjectTaskState::Blocked {}, 2);
    let ended = worker(busy.id, ProjectWorkerState::Ended {}, 5);
    let live = worker(busy.id, ProjectWorkerState::Running {}, 9);
    let gone = worker(idle.id, ProjectWorkerState::Failed {}, 7);
    let live_id = live.id;
    let view = project_view(ProjectParts {
        tasks: vec![idle, busy],
        workers: vec![ended, gone, live],
        ..parts()
    });
    assert_eq!(view.tasks[0].title, "busy");
    assert_eq!(view.tasks[0].worker, Some(live_id));
    assert_eq!(view.tasks[1].worker, None);
}
