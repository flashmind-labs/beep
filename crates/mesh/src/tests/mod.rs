//! Tests that speak the sync stream by hand, as a hostile peer would.

use crate::PeerStream;
use crate::devices::OpBody;
use crate::link::Frame;
use crate::mesh::AddrNote;
use crate::wire::{self, FrameReader};
use crate::{Device, DeviceKind, DeviceOp, Entry, JOIN_ALPN, Key, Mesh, PEER_ALPN};
use iroh::endpoint::{Connection, QuicTransportConfig, SendStream, VarInt, presets};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, SecretKey};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
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
    recv: FrameReader,
    conn: Connection,
    _endpoint: Endpoint,
}

/// Connects as `key` and sends a `Hello` that holds nothing.
async fn raw(to: &Dev, key: &SecretKey) -> Raw {
    raw_in(to, key, fastrand::u64(..)).await
}

/// As `raw`, claiming to be the instance of the peer that has `session`.
async fn raw_in(to: &Dev, key: &SecretKey, session: u64) -> Raw {
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
        session,
    };
    wire::send(&mut send, &hello).await.expect("hello");
    Raw {
        send,
        recv: FrameReader::new(recv, wire::MAX_FRAME),
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
    wire::send(&mut raw.send, &Frame::Ops(vec![add]))
        .await
        .expect("send");
    let key = Key::Credential("c".to_owned());
    let mut forged = Entry::sign(&n, mesh, key.clone(), Some(b"x".to_vec()), 5, 1).expect("signs");
    forged.origin = a.key.public();
    let own = Entry::sign(&n, mesh, key.clone(), Some(b"y".to_vec()), 6, 2).expect("signs");
    wire::send(&mut raw.send, &Frame::Entries(vec![forged, own]))
        .await
        .expect("send");
    // Frames are handled in order: once this note lands, the rest were seen.
    let marker = EndpointAddr::new(n.public())
        .with_ip_addr("127.0.0.1:9".parse::<SocketAddr>().expect("addr"));
    let note = AddrNote {
        addr: marker.clone(),
        generation: u64::MAX,
    };
    wire::send(&mut raw.send, &Frame::Addrs(vec![note]))
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
        let frame = raw
            .recv
            .recv::<Frame>()
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

fn at(id: iroh::EndpointId, port: u16) -> EndpointAddr {
    EndpointAddr::new(id).with_ip_addr(SocketAddr::from(([127, 0, 0, 1], port)))
}

fn known_addr(mesh: &Mesh, id: iroh::EndpointId) -> Option<EndpointAddr> {
    mesh.devices()
        .into_iter()
        .find(|d| d.device.id == id)
        .and_then(|d| d.addr)
}

async fn until_addr(mesh: &Mesh, id: iroh::EndpointId, want: &EndpointAddr) {
    let mut updates = mesh.updates();
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            if known_addr(mesh, id).as_ref() == Some(want) {
                return;
            }
            updates.changed().await.expect("alive");
        }
    })
    .await
    .expect("the address arrived");
}

fn in_a_minute(ms: u64) -> u64 {
    crate::state::now_ms() + ms
}

#[tokio::test]
async fn a_poisoned_relayed_address_is_dropped_and_a_first_hand_one_wins() {
    let a = create().await;
    let relay = member_key(&a, DeviceKind::Desktop).await;
    let victim = member_key(&a, DeviceKind::Desktop).await;
    let mut hostile = raw(&a, &relay).await;
    // Far in the future: dropped.
    let poison = at(victim.public(), 1);
    let note = AddrNote {
        addr: poison.clone(),
        generation: u64::MAX,
    };
    wire::send(&mut hostile.send, &Frame::Addrs(vec![note]))
        .await
        .expect("send");
    // Plausible: kept, as relayed news.
    let near = at(victim.public(), 2);
    let note = AddrNote {
        addr: near.clone(),
        generation: in_a_minute(30_000),
    };
    wire::send(&mut hostile.send, &Frame::Addrs(vec![note]))
        .await
        .expect("send");
    until_addr(&a.mesh, victim.public(), &near).await;
    assert_ne!(known_addr(&a.mesh, victim.public()), Some(poison));
    // The victim's own link beats any relayed counter.
    let mut own = raw(&a, &victim).await;
    let first_hand = at(victim.public(), 3);
    let note = AddrNote {
        addr: first_hand.clone(),
        generation: 1,
    };
    wire::send(&mut own.send, &Frame::Addrs(vec![note]))
        .await
        .expect("send");
    until_addr(&a.mesh, victim.public(), &first_hand).await;
}

#[tokio::test]
async fn a_node_flooding_forged_entries_is_closed() {
    let a = create().await;
    let n = member_key(&a, DeviceKind::Node).await;
    let mut raw = raw(&a, &n).await;
    let mesh = a.key.public();
    for seq in 1..=20 {
        let mut forged = Entry::sign(&n, mesh, Key::Flsh, Some(vec![1]), 5, seq).expect("signs");
        forged.origin = a.key.public();
        wire::send(&mut raw.send, &Frame::Entries(vec![forged]))
            .await
            .expect("send");
    }
    timeout(LIMIT, raw.conn.closed())
        .await
        .expect("the flooding peer was dropped");
}

#[tokio::test]
async fn open_bi_goes_on_to_the_link_that_replaced_a_closed_one() {
    let a = create().await;
    let ticket = a
        .mesh
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .expect("ticket");
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::join(dir.path(), endpoint.clone(), &ticket, "b")
        .await
        .expect("joins");
    let _b = serve(endpoint, &mesh);
    let peer = key.public();
    let mut updates = a.mesh.updates();
    let live = |mesh: &Mesh| mesh.peer(peer);
    let stale = timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            if let Some(conn) = live(&a.mesh) {
                return conn;
            }
            updates.changed().await.expect("alive");
        }
    })
    .await
    .expect("the peer connects");
    // The link is replaced: the old one closes, a new one takes its place.
    stale.close(3u32.into(), b"duplicate");
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            if live(&a.mesh).is_some_and(|c| c.stable_id() != stale.stable_id()) {
                return;
            }
            updates.changed().await.expect("alive");
        }
    })
    .await
    .expect("the link comes back");
    assert!(stale.open_bi().await.is_err());
    a.mesh
        .open_from(peer, stale)
        .await
        .expect("the newer link opens the stream");
}

/// A stream that is open when a second connection from the same peer wins
/// the tie keeps working, and the connection it is on goes once it ends.
#[tokio::test]
async fn a_stream_open_on_the_connection_that_loses_the_tie_survives() {
    let a = create().await;
    // The smaller id's dial wins a tie, so `b` is smaller and its second
    // connection always takes over.
    let key = loop {
        let key = SecretKey::generate();
        if key.public() < a.key.public() {
            break key;
        }
    };
    let ticket = a
        .mesh
        .invite(DeviceKind::Desktop, Duration::from_secs(60))
        .await
        .expect("ticket");
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::join(dir.path(), endpoint.clone(), &ticket, "b")
        .await
        .expect("joins");
    let (peer, join) = mesh.protocols();
    peer.on_stream(Arc::new(|mut stream: PeerStream| {
        Box::pin(async move {
            let mut byte = [0u8; 1];
            while stream.recv.read_exact(&mut byte).await.is_ok() {
                if stream.send.write_all(&byte).await.is_err() {
                    return;
                }
            }
        })
    }));
    let _b = Router::builder(endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn();
    let mut updates = a.mesh.updates();
    let first = timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            if let Some(conn) = a.mesh.peer(key.public()) {
                return conn;
            }
            updates.changed().await.expect("alive");
        }
    })
    .await
    .expect("the peer connects");
    let (mut send, mut recv) = first.open_bi().await.expect("stream");
    let mut byte = [0u8; 1];
    send.write_all(b"x").await.expect("write");
    timeout(LIMIT, recv.read_exact(&mut byte))
        .await
        .expect("answered")
        .expect("read");
    // The same instance dials again: the tie.
    let _second = raw_in(&a, &key, mesh.session()).await;
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            if a.mesh
                .peer(key.public())
                .is_some_and(|c| c.stable_id() != first.stable_id())
            {
                return;
            }
            updates.changed().await.expect("alive");
        }
    })
    .await
    .expect("the second connection takes over");
    send.write_all(b"y")
        .await
        .expect("the stream outlives the tie");
    timeout(LIMIT, recv.read_exact(&mut byte))
        .await
        .expect("answered")
        .expect("the stream outlives the tie");
    assert_eq!(&byte, b"y");
    // With its last stream over, the old connection goes.
    send.finish().expect("finish");
    drop((send, recv));
    timeout(LIMIT, first.closed())
        .await
        .expect("the old connection closes");
}

/// A frame from a newer build, with a field this one lacks after the ones it
/// has, is read like any other: the extra element is skipped.
#[test]
fn a_frame_with_a_field_from_a_newer_build_is_read() {
    use serde::Serialize;
    #[derive(Serialize)]
    enum Newer {
        Hello {
            addrs: Vec<AddrNote>,
            #[serde(with = "crate::wire::heads")]
            heads: crate::Heads,
            session: u64,
            added: String,
        },
    }
    let key = SecretKey::generate();
    let sent = Newer::Hello {
        addrs: vec![AddrNote {
            addr: EndpointAddr::new(key.public()),
            generation: 3,
        }],
        heads: BTreeMap::from([(key.public(), 7)]),
        session: 11,
        added: "later".to_owned(),
    };
    let wire = rmp_serde::to_vec(&sent).expect("encodes");
    let Frame::Hello {
        addrs,
        heads,
        session,
    } = beep_proto::lenient::from_slice(&wire).expect("a newer hello is read")
    else {
        panic!("not a hello");
    };
    assert_eq!((addrs.len(), session), (1, 11));
    assert_eq!(heads.get(&key.public()), Some(&7));
}
