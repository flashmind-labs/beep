//! A request body over a link with a long round trip. A relay in this
//! process delays every datagram, so nothing outside the test changes.

use beep_mesh::{PeerClient, transport_config};
use beep_proto::node::{Open, Opened, link};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::unbounded_channel;
use tokio::time::{Instant, sleep_until};

const ALPN: &[u8] = b"beep-test/latency";
const ONE_WAY: Duration = Duration::from_millis(100);

#[derive(Serialize, Deserialize)]
struct Upload {
    #[serde(with = "serde_bytes")]
    body: Vec<u8>,
}

type Peer = Arc<Mutex<Option<SocketAddr>>>;

/// Sends what `from` receives out of `to` to `dest` after `delay`, in order.
/// With `learn`, the senders seen on `from` become `learned`.
fn pipe(
    from: Arc<UdpSocket>,
    to: Arc<UdpSocket>,
    dest: Peer,
    learned: Option<Peer>,
    delay: Duration,
) {
    let (queue, mut due) = unbounded_channel::<(Instant, Vec<u8>)>();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        while let Ok((n, src)) = from.recv_from(&mut buf).await {
            if let Some(learned) = &learned {
                *learned.lock().unwrap() = Some(src);
            }
            let _ = queue.send((Instant::now() + delay, buf[..n].to_vec()));
        }
    });
    tokio::spawn(async move {
        while let Some((at, data)) = due.recv().await {
            sleep_until(at).await;
            let target = *dest.lock().unwrap();
            if let Some(target) = target {
                let _ = to.send_to(&data, target).await;
            }
        }
    });
}

/// A relay the caller dials as the node's address; returns that address.
async fn relay(server: SocketAddr, delay: Duration) -> SocketAddr {
    let front = Arc::new(UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap());
    let back = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap());
    let front_addr = front.local_addr().unwrap();
    let client: Peer = Arc::new(Mutex::new(None));
    let node: Peer = Arc::new(Mutex::new(Some(server)));
    pipe(
        front.clone(),
        back.clone(),
        node,
        Some(client.clone()),
        delay,
    );
    pipe(back, front, client, None, delay);
    front_addr
}

async fn bind(addr: SocketAddr) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .clear_ip_transports()
        .bind_addr(addr)
        .expect("address is valid")
        .transport_config(transport_config())
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .expect("endpoint binds")
}

/// Serves `Open`, then one `Upload` per stream, answering its length.
async fn serve(node: Endpoint) {
    while let Some(incoming) = node.accept().await {
        tokio::spawn(async move {
            let conn = incoming.await.expect("accepted");
            while let Ok((send, recv)) = conn.accept_bi().await {
                tokio::spawn(async move {
                    let (mut tx, mut rx) = link(send, recv);
                    let _: Open = rx.recv().await.unwrap().unwrap();
                    tx.send(&Opened::Ready {}).await.unwrap();
                    let up: Upload = rx.recv().await.unwrap().unwrap();
                    tx.send(&up.body.len()).await.unwrap();
                    let _ = tx.finish().await;
                });
            }
        });
    }
}

/// A 160 KB request over a link with a 200 ms round trip, direct and with no
/// other path, takes a few round trips, not one per few kilobytes (#1227).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_body_crosses_a_long_link_in_a_few_round_trips() {
    // The caller and the node sit on different address families, so the only
    // way between them is the delaying relay, never a direct path.
    if UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).await.is_err() {
        return;
    }

    let node = bind((Ipv4Addr::LOCALHOST, 0).into()).await;
    let port = node
        .bound_sockets()
        .into_iter()
        .next()
        .expect("bound")
        .port();
    let front = relay((Ipv4Addr::LOCALHOST, port).into(), ONE_WAY).await;
    tokio::spawn(serve(node.clone()));
    let caller = bind((Ipv6Addr::LOCALHOST, 0).into()).await;
    let addr = EndpointAddr::new(node.id()).with_ip_addr(front);
    let conn = caller.connect(addr, ALPN).await.expect("connects");
    let client = PeerClient::new(conn);

    for size in [160_000, 160_000] {
        let started = Instant::now();
        let n: usize = client
            .call_idle(
                Duration::from_secs(30),
                "upload/1",
                &Upload {
                    body: vec![7; size],
                },
            )
            .await
            .expect("upload");
        let round_trips = started.elapsed().as_secs_f64() / (2.0 * ONE_WAY.as_secs_f64());

        assert_eq!(n, size);
        // Slow start needs about six; the report in #1227 was forty.
        assert!(
            round_trips < 20.0,
            "{size} bytes took {round_trips:.1} round trips"
        );
    }
}
