use super::*;
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
        let probed = follower.wait_for(Option::is_some).await.expect("a report");
        assert_eq!(probed.as_ref().expect("probed").report.hostname, "1");
    }

    // A later follower gets the kept report without a probe of its own.
    let third = probes.follow();
    assert!(third.borrow().is_some());
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn the_report_is_probed_again_after_reprobe() {
    let (probes, runs) = counted();
    let mut follower = probes.follow();
    follower.wait_for(Option::is_some).await.expect("a report");

    follower
        .wait_for(|probed| probed.as_ref().is_some_and(|p| p.report.hostname == "2"))
        .await
        .expect("a second report");
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}
