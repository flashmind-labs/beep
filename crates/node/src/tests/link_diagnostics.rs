use super::*;
use iroh::endpoint::presets;

#[tokio::test]
async fn metrics_snapshots_include_transport_and_application_values() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("metrics.log");
    let endpoint = Endpoint::builder(presets::Minimal)
        .bind()
        .await
        .expect("endpoint");
    let task = start(&endpoint, &path).expect("start metrics");
    metrics::counter!("beep_test_requests_total").increment(1);
    let text = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&path)
                && text.contains("beep_test_requests_total 1")
            {
                break text;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("snapshot");
    assert!(text.contains("# snapshot unix_seconds="));
    assert!(text.contains("socket_recv_datagrams_total"));
    drop(task);
    endpoint.close().await;
}
