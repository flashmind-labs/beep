use super::*;
use iroh::{SecretKey, endpoint::presets};

#[tokio::test]
async fn a_failed_dial_forgets_the_peers_lock() {
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let peer = EndpointAddr::new(SecretKey::generate().public());
    let pool = SyncPool::default();

    let lease = timeout(Duration::from_secs(30), pool.acquire(&endpoint, &peer))
        .await
        .expect("the dial gives up on its own");

    assert!(lease.is_err());
    assert!(lock(&pool.dials).is_empty());
}
