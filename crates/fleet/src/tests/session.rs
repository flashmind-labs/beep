use super::*;
use beep_proto::{RunState, WorkflowRun};

#[test]
fn a_failed_read_dates_the_last_one_so_the_next_try_waits_for_its_age() {
    let cache: Cache<String, u8> = Cache::default();
    cache.put("a".into(), 1);
    let hour = Duration::from_secs(3600);
    cache
        .held
        .lock()
        .expect("not poisoned")
        .get_mut("a")
        .expect("held")
        .at = None;
    assert_eq!(cache.get(&"a".to_owned(), hour), Some((1, true)));
    cache.touch(&"a".to_owned());
    assert_eq!(cache.get(&"a".to_owned(), hour), Some((1, false)));
}

#[test]
fn the_oldest_reads_go_past_the_limit() {
    let cache: Cache<u8, u8> = Cache::default();

    for key in 0..4 {
        cache.put_within(key, key, 3);
    }

    let hour = Duration::from_secs(3600);
    assert_eq!(cache.get(&0, hour), None);
    assert!((1..4).all(|key| cache.get(&key, hour).is_some()));
}

fn detail(state: RunState) -> ProjectRunDetail {
    ProjectRunDetail {
        run: WorkflowRun {
            id: 1,
            number: 1,
            workflow: "CI".into(),
            title: String::new(),
            branch: "main".into(),
            event: "push".into(),
            actor: String::new(),
            state,
            url: String::new(),
            started_at: None,
            finished_at: None,
            progress: None,
            pull_requests: Vec::new(),
        },
        jobs: Vec::new(),
        log: None,
        error: None,
        rate_limited_until: None,
    }
}

#[test]
fn a_finished_run_stays_fresh_and_a_running_one_polls_unless_the_limit_is_low() {
    let now = SystemTime::now();
    let fresh = FRESH;
    assert_eq!(
        run_fresh(&detail(RunState::Failed {}), fresh, now),
        RUN_DONE_FRESH
    );
    assert_eq!(
        run_fresh(&detail(RunState::Running {}), fresh, now),
        RUN_GOING_FRESH
    );
    let limited = ProjectRunDetail {
        rate_limited_until: Some(now + Duration::from_secs(600)),
        ..detail(RunState::Running {})
    };
    assert_eq!(run_fresh(&limited, fresh, now), fresh);
    let partly = ProjectRunDetail {
        error: Some("The log did not read".into()),
        ..detail(RunState::Failed {})
    };
    assert_eq!(run_fresh(&partly, fresh, now), fresh);
}

#[test]
fn spent_hear_ignores_timestamp_only_plan_updates() {
    let now = SystemTime::now();
    let plan = |as_of: SystemTime, used_percent: f64| PlanLimits {
        agent: beep_proto::AgentSlug::CLAUDE,
        plan: Some("max".into()),
        machine_name: "host".into(),
        as_of,
        windows: vec![beep_proto::PlanWindow {
            minutes: 300,
            used_percent,
            resets_at: None,
            scope: None,
        }],
        credits: Vec::new(),
        account: None,
    };
    let spent = Spent::new(None);
    assert!(spent.hear(vec![plan(now, 10.0)]));
    assert!(!spent.hear(vec![plan(now + Duration::from_secs(30), 10.0)]));
    assert!(spent.hear(vec![plan(now + Duration::from_secs(60), 15.0)]));
}
