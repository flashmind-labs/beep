//! Real connections between two endpoints: the counters reflect the bytes
//! moved per ALPN, also over iroh-blobs and after a connection ended.

use beep_node::link::Traffic;
use iroh::{
    Endpoint,
    endpoint::{Connection, presets},
    protocol::Router,
};
use iroh_blobs::{BlobsProtocol, store::mem::MemStore};

const ALPN: &[u8] = b"test-traffic/1";
const KEY_TX: &str = "alpn.test-traffic/1.tx";
const KEY_RX: &str = "alpn.test-traffic/1.rx";
const PAYLOAD: usize = 4_000_000;

async fn endpoint(alpns: Vec<Vec<u8>>) -> (Endpoint, Traffic) {
    let traffic = Traffic::default();
    let endpoint = Endpoint::builder(presets::Minimal)
        .alpns(alpns)
        .hooks(traffic.clone())
        .bind()
        .await
        .expect("a test endpoint binds");
    (endpoint, traffic)
}

fn counter(traffic: &Traffic, key: &str) -> u64 {
    traffic.report().counters.get(key).copied().unwrap_or(0)
}

/// `bytes` on the wire carry `moved` bytes of payload: all of it, and no more
/// than a fifth of overhead (packet headers, acks, retransmits on loopback).
fn assert_carries(bytes: u64, moved: usize, what: &str) {
    let moved = moved as u64;
    assert!(bytes >= moved, "{what}: {bytes} under {moved}");
    assert!(
        bytes <= moved + moved / 5,
        "{what}: {bytes} far over {moved}"
    );
}

/// Sends `PAYLOAD` bytes one way over a new connection and returns both ends,
/// the bytes all received.
async fn send_payload(from: &Endpoint, to: &Endpoint) -> (Connection, Connection) {
    let incoming = to.accept();
    let (accepted, sent) = tokio::join!(
        async {
            let connection = incoming
                .await
                .expect("the endpoint is open")
                .await
                .expect("the connection is accepted");
            let mut recv = connection.accept_uni().await.expect("a stream opens");
            let body = recv.read_to_end(PAYLOAD * 2).await.expect("the bytes come");
            assert_eq!(body.len(), PAYLOAD);
            connection
        },
        async {
            let connection = from.connect(to.addr(), ALPN).await.expect("it connects");
            let mut send = connection.open_uni().await.expect("a stream opens");
            send.write_all(&vec![7u8; PAYLOAD]).await.expect("it sends");
            send.finish().expect("the stream finishes");
            send.stopped().await.expect("the peer read it all");
            connection
        },
    );
    (sent, accepted)
}

#[tokio::test]
async fn counts_per_alpn_and_keeps_what_closed_connections_moved() {
    let (client, client_traffic) = endpoint(Vec::new()).await;
    let (server, server_traffic) = endpoint(vec![ALPN.to_vec()]).await;

    let (sent, accepted) = send_payload(&client, &server).await;
    assert_carries(counter(&client_traffic, KEY_TX), PAYLOAD, "sent, open");
    assert_carries(counter(&server_traffic, KEY_RX), PAYLOAD, "received, open");

    // Watching a connection the hook already saw does not count it twice.
    client_traffic.watch(&sent);
    assert_carries(counter(&client_traffic, KEY_TX), PAYLOAD, "watched twice");

    let before = counter(&client_traffic, KEY_TX);
    sent.close(0u32.into(), b"done");
    sent.closed().await;
    accepted.closed().await;
    drop((sent, accepted));
    assert!(counter(&client_traffic, KEY_TX) >= before, "lost at close");
    assert_carries(
        counter(&server_traffic, KEY_RX),
        PAYLOAD,
        "received, closed",
    );

    // A second connection adds to what the first left.
    let (sent, accepted) = send_payload(&client, &server).await;
    assert_carries(counter(&client_traffic, KEY_TX), PAYLOAD * 2, "sent twice");
    assert_carries(
        counter(&server_traffic, KEY_RX),
        PAYLOAD * 2,
        "received twice",
    );
    drop((sent, accepted));

    let report = client_traffic.report();
    assert!(report.started_at_ms > 0);
    assert!(report.counters.contains_key(KEY_RX));

    client.close().await;
    server.close().await;
}

/// iroh-blobs opens and accepts its own connections; the hook attributes them.
#[tokio::test]
async fn counts_a_blob_download() {
    let (provider, provider_traffic) = endpoint(Vec::new()).await;
    let store = MemStore::new();
    let tag = store
        .add_slice(vec![9u8; PAYLOAD])
        .await
        .expect("the blob is stored");
    let router = Router::builder(provider)
        .accept(iroh_blobs::ALPN, BlobsProtocol::new(&store, None))
        .spawn();

    let (downloader, downloader_traffic) = endpoint(Vec::new()).await;
    let connection = downloader
        .connect(router.endpoint().addr(), iroh_blobs::ALPN)
        .await
        .expect("it connects");
    let fetched = MemStore::new();
    fetched
        .remote()
        .fetch(connection.clone(), tag.hash_and_format())
        .await
        .expect("the blob downloads");

    let tx = format!("alpn.{}.tx", String::from_utf8_lossy(iroh_blobs::ALPN));
    let rx = format!("alpn.{}.rx", String::from_utf8_lossy(iroh_blobs::ALPN));
    assert!(counter(&provider_traffic, &tx) >= PAYLOAD as u64);
    assert!(counter(&downloader_traffic, &rx) >= PAYLOAD as u64);

    connection.close(0u32.into(), b"done");
    router.shutdown().await.expect("the router stops");
    downloader.close().await;
}
