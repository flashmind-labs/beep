use super::*;
use flashmind::core::CancellationToken;
use std::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};

fn query() -> DeviceQuery {
    DeviceQuery::Contacts {
        name: "Ana".to_owned(),
    }
}

#[tokio::test]
async fn dropped_device_query_is_removed() {
    let queries = DeviceQueries::default();
    let live = LiveRun::new(CancellationToken::new());
    {
        let mut wait = pin!(ask(&queries, &live, query(), 60));
        poll_fn(|cx| {
            assert!(wait.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(pending(&queries).len(), 1);
    }
    assert!(pending(&queries).is_empty());
}

#[tokio::test]
async fn timed_out_device_query_is_removed() {
    let queries = DeviceQueries::default();
    let live = LiveRun::new(CancellationToken::new());
    assert!(ask(&queries, &live, query(), 0).await.is_err());
    assert!(pending(&queries).is_empty());
}
