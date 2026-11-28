use super::{IDLE, Idle};
use std::time::Duration;
use tokio::time::{interval, sleep};

#[tokio::test(start_paused = true)]
async fn a_sweep_tick_does_not_restart_the_idle_deadline() {
    let mut idle = Idle::new();
    let mut sweep = interval(Duration::from_secs(10));
    let start = tokio::time::Instant::now();

    loop {
        tokio::select! {
            _ = sweep.tick() => {},
            () = idle.expired() => break,
        }
    }

    assert_eq!(start.elapsed(), IDLE);
}

#[tokio::test(start_paused = true)]
async fn a_message_restarts_the_idle_deadline() {
    let mut idle = Idle::new();
    let start = tokio::time::Instant::now();

    sleep(Duration::from_secs(30)).await;
    idle.reset();
    idle.expired().await;

    assert_eq!(start.elapsed(), Duration::from_secs(30) + IDLE);
}
