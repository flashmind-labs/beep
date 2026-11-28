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

/// Accepts a connection and keeps it until the dialer closes it.
#[derive(Debug)]
struct Open;

impl iroh::protocol::ProtocolHandler for Open {
    async fn accept(&self, connection: Connection) -> Result<(), iroh::protocol::AcceptError> {
        connection.closed().await;
        Ok(())
    }
}

#[tokio::test]
async fn a_connection_closes_with_its_last_lease() {
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let host = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let router = iroh::protocol::Router::builder(host)
        .accept(ALPN, Open)
        .spawn();
    let peer = router.endpoint().addr();
    let pool = SyncPool::default();

    let first = pool.acquire(&endpoint, &peer).await.unwrap();
    let second = pool.acquire(&endpoint, &peer).await.unwrap();
    let conn = first.connection().clone();
    let id = conn.stable_id();
    assert_eq!(id, second.connection().stable_id());

    first.mark_broken();
    drop(first);
    assert!(conn.close_reason().is_none(), "a lease is still held");

    drop(second);
    assert!(conn.close_reason().is_some());
    assert!(lock(&pool.peers).is_empty());

    // A lone lease dropped without a fault closes it too.
    let lone = pool.acquire(&endpoint, &peer).await.unwrap();
    let conn = lone.connection().clone();
    assert_ne!(conn.stable_id(), id, "a new connection");
    drop(lone);
    assert!(conn.close_reason().is_some());
}
