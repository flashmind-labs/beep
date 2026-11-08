use super::*;

#[tokio::test]
async fn dropping_the_last_lease_wakes_and_stops_the_shared_reactor() {
    let reactor = Reactor::new().unwrap();
    let weak = Arc::downgrade(&reactor);
    let mut stopped = reactor.stopped.clone();
    drop(reactor);
    assert!(weak.upgrade().is_none());
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stopped.wait_for(|done| *done),
    )
    .await
    .unwrap()
    .unwrap();
}
