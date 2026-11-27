use super::*;
use crate::link::Traffic;
use beep_proto::node::StatsReport;
use beep_proto::watched;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(30);

/// What a sampler follows with: `measure` for the disks, a real tally of traffic.
fn follow(sampler: &Sampler, measure: Measure) -> watch::Receiver<Sample> {
    let traffic = Traffic::default();

    sampler.follow(measure, Box::new(move || traffic.report()))
}

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
    let sampler = Sampler::default();
    let mut followed = follow(&sampler, Box::new(|| Box::pin(std::future::pending())));

    timeout(
        LIMIT,
        watched::wait_for(&mut followed, |sample| sample.traffic.is_some()),
    )
    .await
    .expect("the traffic came")
    .expect("the sampler runs");
    let sample = watched::read_and_update(&mut followed, Clone::clone);

    assert!(sample.stats.is_none());
}

#[tokio::test]
async fn every_stream_follows_the_one_sample() {
    let sampler = Sampler::default();
    let both = || -> Measure { Box::new(|| Box::pin(async { measured() })) };
    let (mut first, mut second) = (follow(&sampler, both()), follow(&sampler, both()));

    for followed in [&mut first, &mut second] {
        timeout(
            LIMIT,
            watched::wait_for(followed, |sample| sample.stats.is_some()),
        )
        .await
        .expect("the stats came")
        .expect("the sampler runs");
    }
}

#[tokio::test]
async fn sampling_stops_with_the_last_stream_and_starts_again() {
    let sampler = Sampler::default();
    let measure = || -> Measure { Box::new(|| Box::pin(async { measured() })) };
    let mut followed = follow(&sampler, measure());
    watched::wait_for(&mut followed, |sample| sample.stats.is_some())
        .await
        .expect("a sample came");
    drop(followed);

    // The task ends on its own, or has already.
    let task = lock(&sampler.0.task).take();
    if let Some(task) = task {
        timeout(LIMIT, task).await.expect("it stopped").ok();
    }

    let mut again = follow(&sampler, measure());
    timeout(
        LIMIT,
        watched::wait_for(&mut again, |sample| sample.stats.is_some()),
    )
    .await
    .expect("it sampled again")
    .expect("the sampler runs");
}
