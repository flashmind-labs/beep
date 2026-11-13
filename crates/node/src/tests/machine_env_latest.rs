use super::*;
use beep_proto::watched;
use std::sync::atomic::AtomicUsize;

/// Probes that count their runs and report `hostname` as the run number.
fn counted() -> (Arc<Probes>, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = runs.clone();
    let probes = Probes::new(move || {
        let run = counter.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            EnvReport {
                hostname: run.to_string(),
                ..EnvReport::default()
            }
        })
    });
    (probes, runs)
}

#[tokio::test(start_paused = true)]
async fn two_followers_share_one_probe() {
    let (probes, runs) = counted();
    let mut first = probes.follow();
    let mut second = probes.follow();

    for follower in [&mut first, &mut second] {
        watched::wait_for(follower, Option::is_some)
            .await
            .expect("a report");
        let hostname = watched::read(follower, |probed| {
            probed.as_ref().map(|probed| probed.report.hostname.clone())
        });
        assert_eq!(hostname.as_deref(), Some("1"));
    }

    // A later follower gets the kept report without a probe of its own.
    let third = probes.follow();
    assert!(watched::read(&third, Option::is_some));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn the_report_is_probed_again_after_reprobe() {
    let (probes, runs) = counted();
    let mut follower = probes.follow();
    watched::wait_for(&mut follower, Option::is_some)
        .await
        .expect("a report");

    watched::wait_for(&mut follower, |probed| {
        probed.as_ref().is_some_and(|p| p.report.hostname == "2")
    })
    .await
    .expect("a second report");
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[test]
fn a_probe_task_lost_with_its_runtime_is_started_again() {
    let (probes, runs) = counted();
    let runtime = || {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a runtime")
    };

    // The first runtime ends before its probing task sees a report taken.
    runtime().block_on(async {
        let _follower = probes.follow();
    });

    runtime().block_on(async {
        let mut follower = probes.follow();
        tokio::time::timeout(
            Duration::from_secs(5),
            watched::wait_for(&mut follower, Option::is_some),
        )
        .await
        .expect("a report in time")
        .expect("a report");
    });
    assert!(runs.load(Ordering::SeqCst) >= 1);
}
