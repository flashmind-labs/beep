use super::*;
use beep_proto::node::StatsReport;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(30);

fn measured() -> StatsReport {
    StatsReport {
        disks: Vec::new(),
        memory_used_mb: 1,
        memory_total_mb: 2,
        load: Some(0.5),
    }
}

#[tokio::test]
async fn a_disk_that_hangs_holds_up_the_disks_and_nothing_else() {
    let sampler = Sampler::measuring(Box::new(|| Box::pin(std::future::pending())));
    let mut followed = sampler.follow(&Traffic::default());

    let sample = timeout(LIMIT, followed.wait_for(|sample| sample.traffic.is_some()))
        .await
        .expect("the traffic came")
        .expect("the sampler runs")
        .clone();

    assert!(sample.stats.is_none());
}

#[tokio::test]
async fn every_stream_follows_the_one_sample() {
    let sampler = Sampler::measuring(Box::new(|| Box::pin(async { measured() })));
    let traffic = Traffic::default();
    let (mut first, mut second) = (sampler.follow(&traffic), sampler.follow(&traffic));

    for followed in [&mut first, &mut second] {
        timeout(LIMIT, followed.wait_for(|sample| sample.stats.is_some()))
            .await
            .expect("the stats came")
            .expect("the sampler runs");
    }
}

#[tokio::test]
async fn sampling_stops_with_the_last_stream_and_starts_again() {
    let sampler = Sampler::measuring(Box::new(|| Box::pin(async { measured() })));
    let traffic = Traffic::default();
    let mut followed = sampler.follow(&traffic);
    followed
        .wait_for(|sample| sample.stats.is_some())
        .await
        .expect("a sample came");
    drop(followed);

    // The task ends on its own, or has already.
    let task = lock(&sampler.0.task).take();
    if let Some(task) = task {
        timeout(LIMIT, task).await.expect("it stopped").ok();
    }

    let mut again = sampler.follow(&traffic);
    timeout(LIMIT, again.wait_for(|sample| sample.stats.is_some()))
        .await
        .expect("it sampled again")
        .expect("the sampler runs");
}
