use super::*;
use beep_proto::watched;
use std::time::Duration;

#[tokio::test]
async fn dropping_the_last_lease_wakes_and_stops_the_shared_reactor() {
    let reactor = Reactor::new().unwrap();
    let weak = Arc::downgrade(&reactor);
    let mut stopped = reactor.stopped.clone();
    drop(reactor);
    assert!(weak.upgrade().is_none());
    tokio::time::timeout(
        Duration::from_secs(5),
        watched::wait_for(&mut stopped, |done| *done),
    )
    .await
    .unwrap()
    .unwrap();
}
