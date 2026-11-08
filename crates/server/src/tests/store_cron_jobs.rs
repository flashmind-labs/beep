use crate::{
    account::user::UserKey,
    store::{TestDb, test_chat},
};
use chrono::Utc;
use flashmind::cron::{CronJob, JobSchedule};
use serde_json::json;
use uuid::Uuid;

fn job(metadata: serde_json::Value) -> CronJob {
    CronJob {
        id: Uuid::new_v4(),
        schedule: JobSchedule::Cron("0 9 * * *".to_owned()),
        task: "Check-out at 11".to_owned(),
        enabled: true,
        once: false,
        metadata,
        created_at: Utc::now(),
        last_run: None,
    }
}

#[tokio::test]
async fn group_reminders_stay_out_of_the_members_routines() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store opens");
    let alice = UserKey::for_test("alice");
    let g1 = test_chat("g1");
    store
        .create_group(&alice, g1, "Trip", "Ali")
        .await
        .expect("group made");
    let own = job(json!({"user_id": "alice", "label": "Own"}));
    let group = job(json!({"user_id": "alice", "label": "Trip", "group_id": g1.to_string()}));
    store.upsert_cron_job(&own).await.expect("routine saved");
    store.upsert_cron_job(&group).await.expect("reminder saved");

    let mine = store.cron_jobs(Some("alice")).await.expect("jobs load");
    assert_eq!(mine.iter().map(|j| j.id).collect::<Vec<_>>(), [own.id]);
    assert_eq!(
        store.list_crons(&alice).await.expect("routines load").len(),
        1
    );
    assert!(
        !store
            .delete_cron(&alice, &group.id.to_string())
            .await
            .expect("delete runs")
    );

    let all = store.cron_jobs(None).await.expect("jobs load");
    let fired = all
        .iter()
        .find(|j| j.id == group.id)
        .expect("scheduler sees it");
    assert_eq!(fired.metadata["group_id"], g1.to_string());
    assert_eq!(
        store
            .group_reminders(g1)
            .await
            .expect("reminders load")
            .len(),
        1
    );

    assert!(
        !store
            .delete_group_reminder(test_chat("g2"), group.id)
            .await
            .expect("delete runs")
    );
    assert!(
        store
            .delete_group_reminder(g1, group.id)
            .await
            .expect("delete runs")
    );
    assert!(
        store
            .group_reminders(g1)
            .await
            .expect("reminders load")
            .is_empty()
    );
}
