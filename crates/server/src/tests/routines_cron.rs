use super::*;
use crate::store::TestDb;
use chrono::{TimeDelta, Utc};
use flashmind::{cron::JobSchedule, types::tool::ToolContext};
use std::time::Duration;

#[test]
fn a_job_that_runs_once_is_always_sent() {
    let job = |once| CronJob {
        id: Uuid::new_v4(),
        schedule: JobSchedule::Once(Utc::now()),
        task: "ping".to_owned(),
        enabled: true,
        once,
        metadata: json!({"user_id": "user"}),
        created_at: Utc::now(),
        last_run: None,
    };
    assert!(always_sent(&job(true)));
    assert!(!always_sent(&job(false)));
}

#[tokio::test]
async fn a_reminder_from_the_tools_wakes_the_scheduler() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store opens");
    let changed = store.crons_changed();
    let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store, "user")));

    let at = Utc::now() + TimeDelta::minutes(1);
    tools
        .create_with_metadata(
            JobSchedule::Once(at),
            "ping".to_owned(),
            true,
            json!({"user_id": "user"}),
        )
        .await
        .expect("reminder saved");

    tokio::time::timeout(Duration::from_secs(1), changed.notified())
        .await
        .expect("saving a reminder notifies the scheduler");
}

#[tokio::test]
async fn no_tool_that_sets_up_routines_is_left() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store opens");
    let mut registry = ToolRegistry::new();
    register_tools(&mut registry, store, "user", "session", None);
    assert!(registry.contains("cron_create"));
    remove_changing_tools(&mut registry);

    for name in CHANGING_TOOLS {
        assert!(!registry.contains(name), "{name} is gone");
    }

    assert!(registry.contains("cron_list"), "reading routines stays");
}

#[tokio::test]
async fn a_local_reminder_time_is_read_in_the_users_timezone() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store opens");
    let mut registry = ToolRegistry::new();
    register_tools(
        &mut registry,
        store,
        "user",
        "session",
        Some("Asia/Singapore"),
    );
    let tool = registry
        .get("schedule_once")
        .expect("schedule_once registered");

    let cancel = CancellationToken::new();
    let args = json!({"datetime": "2099-10-05T11:30:00", "task": "ping"});
    let result = tool
        .execute(ToolContext::new("call", args, None, &cancel))
        .await
        .expect("tool runs");

    assert!(result.is_success(), "{}", result.output());
    assert!(
        result.output().contains("2099-10-05 03:30 UTC"),
        "{}",
        result.output()
    );
}

#[tokio::test]
async fn a_routine_made_outside_a_chat_keeps_its_first_chat() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store opens");
    let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), "user")));
    let job = tools
        .create_with_metadata(
            JobSchedule::Cron("0 20 * * *".to_owned()),
            "evening digest".to_owned(),
            false,
            json!({"user_id": "user"}),
        )
        .await
        .expect("routine saved");

    let first = store
        .claim_cron_session(job.id, Uuid::new_v4())
        .await
        .expect("claim runs")
        .expect("routine exists");
    let second = store
        .claim_cron_session(job.id, Uuid::new_v4())
        .await
        .expect("claim runs");
    assert_eq!(second, Some(first));
    let jobs = store.cron_jobs(Some("user")).await.expect("jobs load");
    assert_eq!(jobs[0].metadata["session_id"], first.to_string());
}

#[tokio::test]
async fn the_scheduler_never_brings_back_or_undoes_a_routine() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store opens");
    let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), "user")));
    let scheduler = CronRegistry::new(Arc::new(SchedulerStore {
        store: store.clone(),
    }));
    let job = tools
        .create_with_metadata(
            JobSchedule::Cron("*/15 * * * *".to_owned()),
            "Check whether Sam replied".to_owned(),
            false,
            json!({"user_id": "user", "label": "Sam"}),
        )
        .await
        .expect("routine saved");

    // The scheduler read the job, then the user renamed it while it ran.
    let mut ran = scheduler
        .get(job.id)
        .await
        .expect("job loads")
        .expect("job exists");
    let mut renamed = job.clone();
    renamed.metadata["label"] = json!("Sam's reply");
    tools.update(&renamed).await.expect("rename saved");
    ran.last_run = Some(Utc::now());
    scheduler.update(&ran).await.expect("run recorded");
    let now = store
        .cron_job(job.id, None)
        .await
        .expect("job loads")
        .expect("job exists");
    assert_eq!(now.metadata["label"], "Sam's reply");
    assert!(now.last_run.is_some());

    // The run ended its routine before the scheduler recorded it.
    let tool = EndRoutineTool::new(store.clone(), "user", job.id);
    let cancel = CancellationToken::new();
    tool.execute(ToolContext::new("call", json!({}), None, &cancel))
        .await
        .expect("tool runs");
    scheduler.update(&ran).await.expect("run recorded");
    assert!(
        store
            .cron_job(job.id, None)
            .await
            .expect("job loads")
            .is_none()
    );
}

#[tokio::test]
async fn a_routine_ends_itself_and_no_other() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store opens");
    let routine = |user: &str| {
        let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), user)));
        let user = user.to_owned();
        async move {
            tools
                .create_with_metadata(
                    JobSchedule::Cron("*/15 * * * *".to_owned()),
                    "Check whether Sam replied".to_owned(),
                    false,
                    json!({"user_id": user}),
                )
                .await
                .expect("routine saved")
        }
    };
    let running = routine("user").await;
    let other = routine("user").await;
    let theirs = routine("bob").await;
    let tool = EndRoutineTool::new(store.clone(), "user", running.id);

    // Naming another routine still ends only the one running.
    let cancel = CancellationToken::new();
    let args = json!({"id": other.id.to_string()});
    let result = tool
        .execute(ToolContext::new("call", args, None, &cancel))
        .await
        .expect("tool runs");
    assert!(result.is_success(), "{}", result.output());

    let left: Vec<Uuid> = store
        .cron_jobs(None)
        .await
        .expect("jobs load")
        .into_iter()
        .map(|job| job.id)
        .collect();
    assert!(!left.contains(&running.id));
    assert!(left.contains(&other.id));
    assert!(left.contains(&theirs.id));

    // A run bound to someone else's routine cannot end it.
    let foreign = EndRoutineTool::new(store.clone(), "user", theirs.id);
    foreign
        .execute(ToolContext::new("call", json!({}), None, &cancel))
        .await
        .expect("tool runs");
    let theirs_left = store.cron_job(theirs.id, None).await.expect("job loads");
    assert!(theirs_left.is_some());
}
