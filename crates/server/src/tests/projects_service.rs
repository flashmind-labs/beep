use super::*;
use crate::memory::watchers::testing::{Harness, Scripted};
use anyhow::anyhow;

use tokio::sync::{Semaphore, oneshot};
use uuid::Uuid;

#[tokio::test]
async fn jobs_dropped_for_a_full_queue_are_armed_again() {
    let h = Harness::new(Scripted::default()).await;
    let (app, projects) = (&h.app, &h.app.projects);
    let gate = Arc::new(Semaphore::new(0));
    let (done, mut finished) = mpsc::unbounded_channel();
    let tries = JOBS_RUNNING + JOBS_QUEUED + 8;
    let mut queued = 0;
    for _ in 0..tries {
        let (gate, done) = (gate.clone(), done.clone());
        let job = Job::new(JobKind::Check, move |_| async move {
            // Closed, the gate lets everyone through.
            drop(gate.acquire().await);
            done.send(()).map_err(|_| anyhow!("the test stopped"))
        });
        if projects.spawn(app, job) {
            queued += 1;
        }
    }
    assert!(queued < tries, "{queued} of {tries} queued");
    let now = Utc::now();
    let check = Due::Check(ProjectId(Uuid::new_v4()));
    projects.run_due(app, check, now);
    assert_eq!(projects.armed_at(&check), Some(now + DROPPED_RETRY));
    // A worker's report waits in its worker's queue, which is armed.
    let worker = WorkerId(Uuid::new_v4());
    let (ran, report) = oneshot::channel();
    let job = Job::new(JobKind::Post, move |_| async move {
        ran.send(()).map_err(|()| anyhow!("the test stopped"))
    });
    projects.spawn_for(app, worker, job);
    let retry = projects
        .armed_at(&Due::Worker(worker))
        .expect("the worker's jobs are armed");
    gate.close();
    for _ in 0..queued {
        finished.recv().await.expect("a job finishes");
    }
    for due in projects.take_due(retry) {
        projects.run_due(app, due, retry);
    }
    report.await.expect("the report ran");
}

#[tokio::test]
async fn a_workers_jobs_run_in_order() {
    let h = Harness::new(Scripted::default()).await;
    let (app, projects) = (&h.app, &h.app.projects);
    let worker = WorkerId(Uuid::new_v4());
    let order = Arc::new(Mutex::new(Vec::new()));
    let (started, first_started) = oneshot::channel();
    let (release, gate) = oneshot::channel::<()>();
    let (done, second_done) = oneshot::channel();
    let first = {
        let order = order.clone();
        Job::new(JobKind::Post, move |_| async move {
            started.send(()).map_err(|()| anyhow!("the test stopped"))?;
            gate.await?;
            lock(&order).push(1);
            Ok(())
        })
    };
    let second = {
        let order = order.clone();
        Job::new(JobKind::IdleClose, move |_| async move {
            lock(&order).push(2);
            done.send(()).map_err(|()| anyhow!("the test stopped"))
        })
    };
    projects.spawn_for(app, worker, first);
    projects.spawn_for(app, worker, second);
    first_started.await.expect("the first job starts");
    release.send(()).expect("the first job waits");
    second_done.await.expect("the second job runs");
    assert_eq!(*lock(&order), vec![1, 2]);
}
