use super::*;
use crate::store::TestDb;
use flashmind::cron::JobSchedule;
use serde_json::json;
use uuid::Uuid;

fn digest_job(user: &str) -> CronJob {
    CronJob {
        id: Uuid::new_v4(),
        schedule: JobSchedule::Cron("0 18 * * *".to_owned()),
        task: "digest".to_owned(),
        enabled: true,
        once: false,
        metadata: json!({"user_id": user, "label": "Evening digest", "kind": DIGEST_KIND}),
        created_at: Utc::now(),
        last_run: None,
    }
}

#[tokio::test]
async fn the_digest_is_offered_once_and_stays_deleted() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");

    let job = digest_job("alice");
    assert!(store.offer_digest(&alice, &job).await.unwrap());
    assert!(store.digest_enabled(&alice).await.unwrap());
    let saved = store
        .cron_job(job.id, Some("alice"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.metadata["kind"], DIGEST_KIND);

    // Editing keeps the kind; switching it off turns flagging off.
    let mut off = saved.clone();
    off.enabled = false;
    off.metadata["kind"] = json!(null);
    store.upsert_cron_job(&off).await.unwrap();
    assert!(!store.digest_enabled(&alice).await.unwrap());
    let saved = store
        .cron_job(job.id, Some("alice"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.metadata["kind"], DIGEST_KIND);

    store.delete_cron_job(job.id, Some("alice")).await.unwrap();
    assert!(
        !store
            .offer_digest(&alice, &digest_job("alice"))
            .await
            .unwrap()
    );
    assert!(store.cron_jobs(Some("alice")).await.unwrap().is_empty());
}
