//! Real iroh endpoints on loopback, relays off. Waits ride on `updates()`
//! and outer timeouts; nothing sleeps.

use beep_mesh::{DeviceKind, JOIN_ALPN, Key, Mesh, MeshError, PEER_ALPN};
use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointId, SecretKey};
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

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

async fn join_with(ticket: &beep_mesh::JoinTicket, name: &str) -> Result<Dev, MeshError> {
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
    assert_eq!(changes.recv().await.expect("a change came"), project("p"));
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
