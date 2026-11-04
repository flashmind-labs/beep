//! Tests that speak the sync stream by hand, as a hostile peer would.

use crate::devices::OpBody;
use crate::frame;
use crate::link::Frame;
use crate::mesh::AddrNote;
use crate::{Device, DeviceKind, DeviceOp, Entry, JOIN_ALPN, Key, Mesh, PEER_ALPN};
use iroh::endpoint::{Connection, QuicTransportConfig, RecvStream, SendStream, VarInt, presets};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, SecretKey};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(20);

struct Dev {
    mesh: Mesh,
    router: Router,
    key: SecretKey,
    _dir: TempDir,
}

async fn bind(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        // A small send buffer, so an unread stream blocks the writer.
        .transport_config(QuicTransportConfig::builder().send_window(65536).build())
        .bind()
        .await
        .expect("endpoint binds")
}

fn serve(endpoint: Endpoint, mesh: &Mesh) -> Router {
    let (peer, join) = mesh.protocols();
    Router::builder(endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn()
}

async fn create() -> Dev {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "a")
        .await
        .expect("mesh");
    Dev {
        router: serve(endpoint, &mesh),
        mesh,
        key,
        _dir: dir,
    }
}

/// A member of `from`'s mesh, then shut down, leaving only its key.
async fn member_key(from: &Dev, kind: DeviceKind) -> SecretKey {
    let ticket = from
        .mesh
        .invite(kind, Duration::from_secs(60))
        .await
        .expect("ticket");
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::join(dir.path(), endpoint, &ticket, "m")
        .await
        .expect("joins");
    drop(mesh);
    key
}

struct Raw {
    send: SendStream,
    recv: RecvStream,
    conn: Connection,
    _endpoint: Endpoint,
}

/// Connects as `key` and sends a `Hello` that holds nothing.
async fn raw(to: &Dev, key: &SecretKey) -> Raw {
    // Tiny windows: whatever the other side sends piles up unread.
    let tiny = QuicTransportConfig::builder()
        .stream_receive_window(VarInt::from_u32(4096))
        .receive_window(VarInt::from_u32(8192))
        .build();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .transport_config(tiny)
        .bind()
        .await
        .expect("endpoint binds");
    let conn = endpoint
        .connect(to.router.endpoint().addr(), PEER_ALPN)
        .await
        .expect("connects");
    let (mut send, recv) = conn.open_bi().await.expect("stream");
    let hello = Frame::Hello {
        addrs: Vec::new(),
        heads: BTreeMap::new(),
        session: fastrand::u64(..),
    };
    frame::send(&mut send, &hello).await.expect("hello");
    Raw {
        send,
        recv,
        conn,
        _endpoint: endpoint,
    }
}

#[tokio::test]
async fn a_node_peer_cannot_forge_an_add_or_an_entry() {
    let a = create().await;
    let n = member_key(&a, DeviceKind::Node).await;
    let mut raw = raw(&a, &n).await;
    let mesh = a.key.public();
    let evil = SecretKey::generate();
    let add = DeviceOp::sign(
        &n,
        mesh,
        OpBody::Add(Device {
            id: evil.public(),
            name: "evil".to_owned(),
            kind: DeviceKind::Desktop,
            added_at_ms: 1,
        }),
    )
    .expect("signs");
    frame::send(&mut raw.send, &Frame::Ops(vec![add]))
        .await
        .expect("send");
    let key = Key::Credential("c".to_owned());
    let mut forged = Entry::sign(&n, mesh, key.clone(), Some(b"x".to_vec()), 5, 1).expect("signs");
    forged.origin = a.key.public();
    let own = Entry::sign(&n, mesh, key.clone(), Some(b"y".to_vec()), 6, 2).expect("signs");
    frame::send(&mut raw.send, &Frame::Entries(vec![forged, own]))
        .await
        .expect("send");
    // Frames are handled in order: once this note lands, the rest were seen.
    let marker = EndpointAddr::new(n.public())
        .with_ip_addr("127.0.0.1:9".parse::<SocketAddr>().expect("addr"));
    let note = AddrNote {
        addr: marker.clone(),
        generation: u64::MAX,
    };
    frame::send(&mut raw.send, &Frame::Addrs(vec![note]))
        .await
        .expect("send");
    let mut updates = a.mesh.updates();
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            let seen = a
                .mesh
                .devices()
                .iter()
                .any(|d| d.device.id == n.public() && d.addr.as_ref() == Some(&marker));
            if seen {
                return;
            }
            updates.changed().await.expect("alive");
        }
    })
    .await
    .expect("the marker arrived");
    assert_eq!(a.mesh.devices().len(), 2);
    assert_eq!(a.mesh.get(&key), None);
    drop(raw.recv);
}

#[tokio::test]
async fn a_removed_peer_that_stops_reading_is_closed() {
    let a = create().await;
    let b = member_key(&a, DeviceKind::Desktop).await;
    for i in 0..4 {
        a.mesh
            .set(Key::Project(format!("p{i}")), Some(vec![7u8; 1024 * 1024]))
            .await
            .expect("write");
    }
    let mut raw = raw(&a, &b).await;
    // The peer never reads, so the writer is stuck mid-send. Wait until it is
    // registered, then remove it.
    let mut updates = a.mesh.updates();
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            if a.mesh.peer(b.public()).is_some() {
                return;
            }
            updates.changed().await.expect("alive");
        }
    })
    .await
    .expect("registered");
    // Read up to the first page of entries: the writer is then in the middle
    // of the next one, and nothing more gets read.
    loop {
        let frame = frame::recv::<Frame>(&mut raw.recv, frame::MAX_FRAME)
            .await
            .expect("frame")
            .expect("open");
        if matches!(frame, Frame::Entries(_)) {
            break;
        }
    }
    a.mesh.remove(b.public()).await.expect("removes");
    let why = timeout(LIMIT, raw.conn.closed())
        .await
        .expect("the stalled peer was cut off");
    assert!(why.to_string().contains("closed"), "{why}");
    drop((raw.send, raw.recv));
}
