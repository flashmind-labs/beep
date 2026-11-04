//! Real iroh endpoints on loopback, relays off. Waits ride on `updates()`
//! and outer timeouts; nothing sleeps.

use beep_mesh::{
    DEADLINE, DeviceKind, JOIN_ALPN, JoinTicket, Key, Mesh, MeshError, MeshProtocol, PEER_ALPN,
    PeerClient, PeerStream, SessionFn, StreamHandler, transport_config, with_deadline,
};
use beep_proto::node::{LinkRx, LinkTx, Open, Opened, link};
use futures::StreamExt;
use iroh::endpoint::{Connection, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use std::fs::{copy, create_dir_all, read_dir};
use std::future::pending as never;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::time::{Instant, pause, timeout};

const LIMIT: Duration = Duration::from_secs(60);

struct Dev {
    mesh: Mesh,
    router: Router,
    key: SecretKey,
    dir: TempDir,
}

impl Dev {
    fn id(&self) -> EndpointId {
        self.key.public()
    }

    async fn stop(self) -> (SecretKey, TempDir) {
        self.router.shutdown().await.expect("router stops");
        drop(self.mesh);
        (self.key, self.dir)
    }
}

async fn bind(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("endpoint binds")
}

fn serve(endpoint: Endpoint, mesh: Mesh) -> Router {
    let (peer, join) = mesh.protocols();
    Router::builder(endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn()
}

async fn create(name: &str) -> Dev {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::create(dir.path(), endpoint.clone(), name)
        .await
        .expect("mesh is created");
    let router = serve(endpoint, mesh.clone());
    Dev {
        mesh,
        router,
        key,
        dir,
    }
}

async fn try_join(from: &Dev, kind: DeviceKind, name: &str) -> Result<Dev, MeshError> {
    let ticket = from
        .mesh
        .invite(kind, Duration::from_secs(3600))
        .await
        .expect("ticket is minted");
    join_with(&ticket, name).await
}

async fn join_with(ticket: &JoinTicket, name: &str) -> Result<Dev, MeshError> {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::join(dir.path(), endpoint.clone(), ticket, name).await?;
    let router = serve(endpoint, mesh.clone());
    Ok(Dev {
        mesh,
        router,
        key,
        dir,
    })
}

async fn join(from: &Dev, kind: DeviceKind, name: &str) -> Dev {
    try_join(from, kind, name).await.expect("device joins")
}

async fn reopen(key: SecretKey, dir: TempDir) -> Dev {
    let endpoint = bind(&key).await;
    let mesh = Mesh::open(dir.path(), endpoint.clone())
        .await
        .expect("mesh opens")
        .expect("this directory is in a mesh");
    let router = serve(endpoint, mesh.clone());
    Dev {
        mesh,
        router,
        key,
        dir,
    }
}

async fn until(mesh: &Mesh, ok: impl Fn(&Mesh) -> bool) {
    let mut updates = mesh.updates();
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            if ok(mesh) {
                return;
            }
            updates.changed().await.expect("mesh is alive");
        }
    })
    .await
    .expect("the condition came true in time");
}

fn online(mesh: &Mesh, id: EndpointId) -> bool {
    mesh.devices().iter().any(|d| d.device.id == id && d.online)
}

fn listed(mesh: &Mesh) -> usize {
    mesh.devices().len()
}

fn project(name: &str) -> Key {
    Key::Project(name.to_owned())
}

#[tokio::test]
async fn a_third_device_joins_through_one_member_and_reaches_the_other() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    let c = join(&a, DeviceKind::Node, "c").await;
    until(&b.mesh, |m| listed(m) == 3 && online(m, c.id())).await;
    until(&c.mesh, |m| listed(m) == 3 && online(m, b.id())).await;
    let kinds: Vec<_> = c.mesh.devices().iter().map(|d| d.device.kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == DeviceKind::Node).count(), 1);

    let mut changes = c.mesh.changes();
    b.mesh
        .set(project("p"), Some(b"one".to_vec()))
        .await
        .expect("desktop writes");
    until(&c.mesh, |m| {
        m.get(&project("p")).as_deref() == Some(&b"one"[..])
    })
    .await;
    let key = timeout(LIMIT, changes.recv())
        .await
        .expect("a change came in time")
        .expect("channel open");
    assert_eq!(key, project("p"));
}

#[tokio::test]
async fn a_ticket_works_once() {
    let a = create("a").await;
    let ticket = a
        .mesh
        .invite(DeviceKind::Desktop, Duration::from_secs(3600))
        .await
        .expect("ticket");
    let _b = join_with(&ticket, "b").await.expect("first use works");
    let before = a.mesh.devices().len();
    let again = join_with(&ticket, "x").await;
    match again {
        Err(MeshError::Refused(message)) => assert_eq!(message, "ticket not valid"),
        other => panic!("second use must be refused, got {:?}", other.map(|_| ())),
    }
    assert_eq!(a.mesh.devices().len(), before);
}

#[tokio::test]
async fn two_joiners_racing_one_ticket_have_exactly_one_winner() {
    let a = create("a").await;
    let ticket = a
        .mesh
        .invite(DeviceKind::Desktop, Duration::from_secs(3600))
        .await
        .expect("ticket");
    let (x, y) = tokio::join!(join_with(&ticket, "x"), join_with(&ticket, "y"));
    assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1);
    assert_eq!(a.mesh.devices().len(), 2);
}

#[tokio::test]
async fn an_expired_ticket_is_refused() {
    let a = create("a").await;
    let ticket = a
        .mesh
        .invite(DeviceKind::Desktop, Duration::ZERO)
        .await
        .expect("ticket");
    match join_with(&ticket, "late").await {
        Err(MeshError::Refused(message)) => assert_eq!(message, "ticket expired"),
        other => panic!("expired must be refused, got {:?}", other.map(|_| ())),
    }
    assert_eq!(a.mesh.devices().len(), 1);
}

#[tokio::test]
async fn a_used_ticket_stays_used_after_the_inviter_restarts() {
    let a = create("a").await;
    let ticket = a
        .mesh
        .invite(DeviceKind::Desktop, Duration::from_secs(3600))
        .await
        .expect("ticket");
    let _b = join_with(&ticket, "b").await.expect("first use works");
    let (key, dir) = a.stop().await;
    let a = reopen(key, dir).await;
    // The restarted endpoint listens elsewhere, so point the old ticket at it.
    let mut ticket = ticket;
    ticket.addr = a.router.endpoint().addr();
    assert!(matches!(
        join_with(&ticket, "x").await,
        Err(MeshError::Refused(_))
    ));
    assert_eq!(a.mesh.devices().len(), 2);
}

#[tokio::test]
async fn a_change_reaches_a_device_that_was_off_when_it_came_back() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&a.mesh, |m| online(m, b.id())).await;
    let (key, dir) = b.stop().await;
    until(&a.mesh, |m| {
        m.devices()
            .iter()
            .all(|d| d.device.kind != DeviceKind::Desktop || d.online == (d.device.id == a.id()))
    })
    .await;
    a.mesh
        .set(project("later"), Some(b"x".to_vec()))
        .await
        .expect("write");
    let b = reopen(key, dir).await;
    until(&b.mesh, |m| m.get(&project("later")).is_some()).await;
}

#[tokio::test]
async fn a_change_made_while_the_first_inviter_is_closed_still_spreads() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    let c = join(&a, DeviceKind::Desktop, "c").await;
    until(&b.mesh, |m| online(m, c.id())).await;
    until(&c.mesh, |m| online(m, b.id())).await;
    let a_id = a.id();
    let _ = a.stop().await;
    until(&b.mesh, |m| !online(m, a_id)).await;
    b.mesh
        .set(project("p"), Some(b"1".to_vec()))
        .await
        .expect("write");
    until(&c.mesh, |m| m.get(&project("p")).is_some()).await;
}

#[tokio::test]
async fn a_stranger_is_refused_on_the_peer_alpn() {
    let a = create("a").await;
    let stranger = bind(&SecretKey::generate()).await;
    let connection = stranger
        .connect(a.router.endpoint().addr(), PEER_ALPN)
        .await
        .expect("the handshake itself works");
    let reason = timeout(LIMIT, connection.closed()).await.expect("closed");
    assert!(reason.to_string().contains("not a member"), "{reason}");
    assert!(a.mesh.peer(stranger.id()).is_none());
    assert_eq!(a.mesh.devices().len(), 1);
}

#[tokio::test]
async fn a_node_cannot_mint_a_ticket() {
    let a = create("a").await;
    let n = join(&a, DeviceKind::Node, "n").await;
    let other = SecretKey::generate().public();
    assert!(matches!(
        n.mesh
            .invite(DeviceKind::Desktop, Duration::from_secs(60))
            .await,
        Err(MeshError::NotDesktop(_))
    ));
    assert!(matches!(
        n.mesh.remove(other).await,
        Err(MeshError::NotDesktop(_))
    ));
    assert!(matches!(
        n.mesh.set(project("p"), None).await,
        Err(MeshError::NotDesktop(_))
    ));
}

#[tokio::test]
async fn a_node_refuses_a_join_on_the_join_alpn() {
    let a = create("a").await;
    let n = join(&a, DeviceKind::Node, "n").await;
    let caller = bind(&SecretKey::generate()).await;
    let connection = caller
        .connect(n.router.endpoint().addr(), JOIN_ALPN)
        .await
        .expect("the handshake itself works");
    let reason = timeout(LIMIT, connection.closed()).await.expect("closed");
    assert!(reason.to_string().contains("not a desktop"), "{reason}");
    assert_eq!(n.mesh.devices().len(), 2);
}

#[tokio::test]
async fn a_node_ticket_does_not_make_a_desktop() {
    let a = create("a").await;
    let ticket = a
        .mesh
        .invite(DeviceKind::Node, Duration::from_secs(3600))
        .await
        .expect("ticket");
    let mut lie = ticket.clone();
    lie.kind = DeviceKind::Desktop;
    let liar = join_with(&lie, "liar").await.expect("joins");
    let me = liar
        .mesh
        .devices()
        .into_iter()
        .find(|d| d.device.id == liar.id())
        .expect("listed");
    assert_eq!(me.device.kind, DeviceKind::Node);
    assert!(matches!(
        liar.mesh
            .invite(DeviceKind::Desktop, Duration::from_secs(60))
            .await,
        Err(MeshError::NotDesktop(_))
    ));
    assert!(liar.mesh.set(project("p"), None).await.is_err());
}

#[tokio::test]
async fn a_removed_device_is_cut_off_and_learns_why() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&b.mesh, |m| online(m, a.id())).await;
    let live = b.mesh.peer(a.id()).expect("connected to a");
    a.mesh.remove(b.id()).await.expect("a removes b");
    timeout(LIMIT, live.closed()).await.expect("cut off");
    until(&b.mesh, |m| {
        m.devices().iter().all(|d| d.device.id != b.id())
    })
    .await;
    assert!(matches!(
        b.mesh.set(project("p"), None).await,
        Err(MeshError::Removed(_))
    ));
    assert_eq!(a.mesh.devices().len(), 1);
    until(&a.mesh, |m| m.peer(b.id()).is_none()).await;
}

#[tokio::test]
async fn an_oversize_frame_closes_the_connection_and_the_mesh_goes_on() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    let (key, dir) = b.stop().await;
    let raw = bind(&key).await;
    let connection = raw
        .connect(a.router.endpoint().addr(), PEER_ALPN)
        .await
        .expect("connects");
    let (mut send, _recv) = connection.open_bi().await.expect("stream");
    send.write_all(&u32::MAX.to_be_bytes())
        .await
        .expect("write");
    timeout(LIMIT, connection.closed()).await.expect("closed");
    a.mesh
        .set(project("p"), Some(b"ok".to_vec()))
        .await
        .expect("still serving");
    drop(dir);
}

#[tokio::test]
async fn a_removed_device_that_comes_back_learns_it_was_removed() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    let b_id = b.id();
    let (key, dir) = b.stop().await;
    a.mesh.remove(b_id).await.expect("a removes b");
    let b = reopen(key, dir).await;
    until(&b.mesh, |m| m.devices().iter().all(|d| d.device.id != b_id)).await;
    assert!(matches!(
        b.mesh.set(project("p"), None).await,
        Err(MeshError::Removed(_))
    ));
}

fn at(id: EndpointId, port: u16) -> EndpointAddr {
    EndpointAddr::new(id).with_ip_addr(SocketAddr::from(([127, 0, 0, 1], port)))
}

fn addr_of(mesh: &Mesh, id: EndpointId) -> Option<EndpointAddr> {
    mesh.devices()
        .into_iter()
        .find(|d| d.device.id == id)
        .and_then(|d| d.addr)
}

#[tokio::test]
async fn two_members_holding_different_addresses_for_a_third_settle() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    let c = join(&a, DeviceKind::Desktop, "c").await;
    for (x, y) in [(&a, &b), (&a, &c), (&b, &c)] {
        until(&x.mesh, |m| online(m, y.id())).await;
        until(&y.mesh, |m| online(m, x.id())).await;
    }
    let (low, high) = (at(c.id(), 9), at(c.id(), 10));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    b.mesh.hint_addr(low, now + 1_000).await;
    a.mesh.hint_addr(high.clone(), now + 2_000).await;
    until(&b.mesh, |m| addr_of(m, c.id()).as_ref() == Some(&high)).await;
    until(&a.mesh, |m| addr_of(m, c.id()).as_ref() == Some(&high)).await;
    // A full round trip of state passes through every link.
    let before = [&a, &b, &c].map(|d| d.mesh.addr_updates());
    a.mesh
        .set(project("barrier"), Some(b"1".to_vec()))
        .await
        .expect("write");
    until(&b.mesh, |m| m.get(&project("barrier")).is_some()).await;
    until(&c.mesh, |m| m.get(&project("barrier")).is_some()).await;
    assert_eq!([&a, &b, &c].map(|d| d.mesh.addr_updates()), before);
}

/// Counts connections that reach the peer ALPN, then serves them.
#[derive(Clone, Debug)]
struct Counting {
    inner: MeshProtocol,
    seen: Arc<AtomicUsize>,
    tx: UnboundedSender<()>,
}

impl ProtocolHandler for Counting {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        self.seen.fetch_add(1, Ordering::SeqCst);
        self.tx.send(()).ok();
        self.inner.accept(connection).await
    }
}

#[tokio::test]
async fn a_removed_device_stops_dialing() {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "a")
        .await
        .expect("mesh");
    let (peer, join_protocol) = mesh.protocols();
    let (tx, mut rx) = unbounded_channel();
    let counting = Counting {
        inner: peer,
        seen: Default::default(),
        tx,
    };
    let router = Router::builder(endpoint)
        .accept(PEER_ALPN, counting.clone())
        .accept(JOIN_ALPN, join_protocol)
        .spawn();
    let a = Dev {
        mesh,
        router,
        key,
        dir,
    };
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&a.mesh, |m| online(m, b.id())).await;
    a.mesh.remove(b.id()).await.expect("removes");
    until(&b.mesh, |m| {
        m.devices().iter().all(|d| d.device.id != b.id())
    })
    .await;
    // Whatever was in flight lands first.
    while rx.try_recv().is_ok() {}
    let settled = counting.seen.load(Ordering::SeqCst);
    // Absence cannot be waited for: allow two full backoff steps.
    let again = timeout(Duration::from_millis(2500), async {
        while counting.seen.load(Ordering::SeqCst) == settled {
            rx.recv().await;
        }
    })
    .await;
    assert!(again.is_err(), "the removed device dialed again");
}

fn copy_dir(from: &Path, to: &Path) {
    create_dir_all(to).expect("mkdir");
    for entry in read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            copy(entry.path(), target).expect("copy");
        }
    }
}

#[tokio::test]
async fn a_peer_that_restarts_with_the_same_key_reconnects_at_once() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&a.mesh, |m| online(m, b.id())).await;
    // The old process is still there as far as A can tell: its connection
    // has not closed, and a second one arrives from the same key.
    let dir = TempDir::new().expect("temp dir");
    copy_dir(b.dir.path(), dir.path());
    let b2 = reopen(b.key.clone(), dir).await;
    a.mesh
        .set(project("after"), Some(b"1".to_vec()))
        .await
        .expect("write");
    timeout(Duration::from_secs(5), async {
        until(&b2.mesh, |m| m.get(&project("after")).is_some()).await
    })
    .await
    .expect("the restarted peer got the change without an idle timeout");
}

fn echo() -> StreamHandler {
    Arc::new(|mut stream: PeerStream| {
        Box::pin(async move {
            assert_eq!(stream.kind, DeviceKind::Desktop);
            let mut byte = [0u8; 1];
            if stream.recv.read_exact(&mut byte).await.is_ok() {
                stream.send.write_all(&byte).await.ok();
                stream.send.finish().ok();
                stream.send.stopped().await.ok();
            }
        })
    })
}

#[tokio::test]
async fn a_peers_extra_stream_reaches_the_handler() {
    let a = create("a").await;
    let (peer, _) = a.mesh.protocols();
    peer.on_stream(echo());
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&b.mesh, |m| online(m, a.id())).await;
    let conn = b.mesh.peer(a.id()).expect("connected");
    let (mut send, mut recv) = conn.open_bi().await.expect("stream");
    send.write_all(b"x").await.expect("write");
    let mut back = [0u8; 1];
    timeout(LIMIT, recv.read_exact(&mut back))
        .await
        .expect("answered")
        .expect("read");
    assert_eq!(&back, b"x");
}

#[tokio::test]
async fn a_peer_may_have_64_streams_open_and_no_more() {
    let a = create("a").await;
    let (peer, _) = a.mesh.protocols();
    let (entered, mut entries) = unbounded_channel();
    peer.on_stream(Arc::new(move |stream: PeerStream| {
        let entered = entered.clone();
        Box::pin(async move {
            entered.send(()).ok();
            // Hold the stream open until the connection goes.
            let _keep = stream;
            never::<()>().await;
        })
    }));
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&b.mesh, |m| online(m, a.id())).await;
    let conn = b.mesh.peer(a.id()).expect("connected");
    let mut open = Vec::new();
    for _ in 0..64 {
        let (mut send, recv) = conn.open_bi().await.expect("stream");
        send.write_all(b"x").await.expect("write");
        open.push((send, recv));
    }
    for _ in 0..64 {
        timeout(LIMIT, entries.recv())
            .await
            .expect("entered")
            .expect("open");
    }
    let (mut send, mut recv) = conn.open_bi().await.expect("stream");
    send.write_all(b"x").await.expect("write");
    let mut byte = [0u8; 1];
    let refused = timeout(LIMIT, recv.read_exact(&mut byte))
        .await
        .expect("the 65th was answered");
    assert!(refused.is_err(), "the 65th stream was served");
    assert!(entries.try_recv().is_err());
}

/// A stand-in for the node's `session()`.
async fn session(mut tx: LinkTx, mut rx: LinkRx) {
    let Ok(Some(open)) = rx.recv::<Open>().await else {
        return;
    };
    match open.protocol.as_str() {
        "echo/1" => {
            tx.send(&Opened::Ready {}).await.ok();
            if let Ok(Some(n)) = rx.recv::<u32>().await {
                tx.send(&(n + 1)).await.ok();
            }
        }
        "count/1" => {
            tx.send(&Opened::Ready {}).await.ok();
            if let Ok(Some(n)) = rx.recv::<u32>().await {
                for i in 0..n {
                    tx.send(&i).await.ok();
                }
            }
        }
        "stall/1" => {
            tx.send(&Opened::Ready {}).await.ok();
            if let Ok(Some(_)) = rx.recv::<u32>().await {
                tx.send(&7u32).await.ok();
                never::<()>().await;
            }
        }
        other => {
            let reason = format!("this node does not offer {other}");
            tx.send(&Opened::Refused { reason }).await.ok();
        }
    }
}

fn session_fn() -> SessionFn {
    Arc::new(|tx, rx| Box::pin(session(tx, rx)))
}

fn session_handler() -> StreamHandler {
    Arc::new(|stream: PeerStream| {
        Box::pin(async move {
            let (tx, rx) = link(stream.send, stream.recv);
            session(tx, rx).await;
        })
    })
}

async fn count(client: &PeerClient, n: u32) -> Vec<u32> {
    client
        .follow::<u32, u32>("count/1", &n)
        .await
        .expect("opened")
        .map(|item| item.expect("item"))
        .collect()
        .await
}

#[tokio::test]
async fn the_local_client_and_the_network_client_answer_alike() {
    let a = create("a").await;
    let (peer, _) = a.mesh.protocols();
    peer.on_stream(session_handler());
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&b.mesh, |m| online(m, a.id())).await;
    let net = with_deadline(a.id(), &b.mesh).await.expect("online");
    let local = PeerClient::local(session_fn());
    for client in [&net, &local] {
        let reply: u32 = client.call("echo/1", &41u32).await.expect("answered");
        assert_eq!(reply, 42);
        assert_eq!(count(client, 4).await, [0, 1, 2, 3]);
    }
    let refused = |client: &PeerClient| {
        let client = client.clone();
        async move {
            client
                .open("nope/1")
                .await
                .err()
                .expect("refused")
                .to_string()
        }
    };
    assert_eq!(refused(&net).await, refused(&local).await);
}

#[tokio::test]
async fn follow_ends_with_the_error_when_the_connection_drops() {
    let a = create("a").await;
    let (peer, _) = a.mesh.protocols();
    peer.on_stream(session_handler());
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&b.mesh, |m| online(m, a.id())).await;
    let net = with_deadline(a.id(), &b.mesh).await.expect("online");
    let mut items = Box::pin(net.follow::<u32, u32>("stall/1", &0).await.expect("opened"));
    let first = timeout(LIMIT, items.next()).await.expect("first item");
    assert_eq!(first.expect("an item").expect("ok"), 7);
    a.stop().await;
    let next = timeout(LIMIT, items.next()).await.expect("did not hang");
    assert!(matches!(next, Some(Err(_))), "{next:?}");
    assert!(timeout(LIMIT, items.next()).await.expect("ended").is_none());
}

#[tokio::test]
async fn a_peer_that_is_off_says_so_within_the_deadline() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&a.mesh, |m| online(m, b.id())).await;
    let gone = b.id();
    b.stop().await;
    until(&a.mesh, |m| !online(m, gone)).await;
    pause();
    let start = Instant::now();
    let offline = with_deadline(gone, &a.mesh).await.expect_err("offline");
    // The paused clock rounds a sleep up to the next millisecond.
    let limit = DEADLINE + Duration::from_millis(2);
    assert!(start.elapsed() <= limit, "{:?}", start.elapsed());
    assert!(offline.to_string().contains("offline"));
}

#[tokio::test]
async fn a_removed_device_is_offline_at_once() {
    let a = create("a").await;
    let b = join(&a, DeviceKind::Desktop, "b").await;
    until(&a.mesh, |m| online(m, b.id())).await;
    a.mesh.remove(b.id()).await.expect("removed");
    pause();
    let start = Instant::now();
    with_deadline(b.id(), &a.mesh).await.expect_err("offline");
    assert_eq!(start.elapsed(), Duration::ZERO);
}
