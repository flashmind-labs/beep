use super::*;
use beep_mesh::{
    DEADLINE, DeviceKind, JOIN_ALPN, Key, Mesh, PEER_ALPN, transport_config, with_deadline,
};
use beep_node::machine::provision::Provision;
use beep_proto::node::{
    files,
    harness::HarnessConfig,
    mesh_state::StateValue,
    status::Report,
    tools::{self, Reply, Request, Tool},
};
use beep_proto::{SessionId, watched};
use iroh::{SecretKey, endpoint::presets, protocol::Router};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio::task::JoinSet;

pub(super) const LIMIT: Duration = Duration::from_secs(60);

/// An endpoint on loopback with no relay and no discovery: nothing here
/// can reach a network.
pub(super) async fn bind(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds")
}

pub(super) fn offers(dir: &TempDir, endpoint: &Endpoint, name: &str) -> Bound {
    beep_node::link::Offers::local(
        beep_node::agents::Parts::default(),
        Provision::new(dir.path()),
    )
    .bound(endpoint, dir.path(), name)
}

pub(super) struct Mac {
    pub(super) fleet: Arc<Fleet>,
    pub(super) endpoint: Endpoint,
    pub(super) dir: TempDir,
}

/// A Mac that has started, with its mesh open and serving.
pub(super) async fn start(name: &str) -> Mac {
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&SecretKey::generate()).await;
    let fleet = Arc::new(
        Fleet::new(
            endpoint.clone(),
            dir.path(),
            name,
            offers(&dir, &endpoint, name),
        )
        .await,
    );
    fleet.run().await;
    Mac {
        fleet,
        endpoint,
        dir,
    }
}

#[tokio::test]
async fn a_removed_terminal_ends_its_saved_tab_and_can_be_closed_again() {
    let mac = start("build-box").await;
    let id = uuid::Uuid::new_v4();
    let opened = terminals::attach(&mac.fleet, id, 0, 100, 30)
        .await
        .expect("a missing terminal reports its end");
    assert!(matches!(
        opened,
        terminals::Opened::Over(beep_proto::TerminalEvent::Ended {
            state: beep_proto::TerminalState::Gone {}
        })
    ));
    terminals::close(&mac.fleet, id)
        .await
        .expect("closing a removed terminal is idempotent");
    terminals::close(&mac.fleet, id)
        .await
        .expect("closing it again succeeds");
}

pub(super) async fn until(mesh: &Mesh, ok: impl Fn(&Mesh) -> bool) {
    let mut updates = mesh.updates();
    timeout(LIMIT, async {
        loop {
            watched::read_and_update(&mut updates, |_| ());

            if ok(mesh) {
                return;
            }

            updates.changed().await.expect("the mesh is alive");
        }
    })
    .await
    .expect("the condition came true in time");
}

pub(super) fn online(mesh: &Mesh, id: EndpointId) -> bool {
    mesh.devices().iter().any(|d| d.device.id == id && d.online)
}

/// A device that joins the mesh of `host` as `kind`, serving its own
/// side of the mesh so the two connect.
pub(super) async fn join(host: &Fleet, kind: DeviceKind, name: &str) -> (Mesh, Router, Endpoint) {
    join_serving(host, kind, name, None).await
}

async fn join_serving(
    host: &Fleet,
    kind: DeviceKind,
    name: &str,
    commands: Option<SessionsProtocol>,
) -> (Mesh, Router, Endpoint) {
    let ticket = host
        .mesh()
        .expect("the host is in a mesh")
        .invite(kind, Duration::from_secs(3600))
        .await
        .expect("a ticket is minted");
    let dir = TempDir::new().expect("a temp dir").keep();
    let endpoint = bind(&SecretKey::generate()).await;
    let mesh = Mesh::join(&dir, endpoint.clone(), &ticket, name)
        .await
        .expect("the device joins");
    let (peer, join) = mesh.protocols();
    let mut router = Router::builder(endpoint.clone())
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join);
    if let Some(commands) = commands {
        router = router.accept(beep_proto::node::commands::ALPN, commands);
    }
    let router = router.spawn();
    (mesh, router, endpoint)
}

#[tokio::test]
async fn the_app_starts_with_no_server_and_makes_a_mesh() {
    let mac = start("Studio").await;
    let machines = mac.fleet.machines();
    assert_eq!(machines.len(), 1, "{machines:?}");
    let this = &machines[0];
    assert_eq!(this.device.id, mac.endpoint.id());
    assert_eq!(this.device.kind, DeviceKind::Desktop);
    assert!(this.online);
    assert!(mac.dir.path().join("mesh2").join("devices.bin").exists());
    // The next launch opens the same mesh, not a new one.
    mac.fleet.stop().await;
    let again = Fleet::new(
        mac.endpoint.clone(),
        mac.dir.path(),
        "Studio",
        offers(&mac.dir, &mac.endpoint, "Studio"),
    )
    .await;
    again.run().await;
    assert_eq!(again.machines().len(), 1);
    again.stop().await;
}

#[tokio::test]
async fn a_call_to_this_mac_does_not_touch_the_network() {
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&SecretKey::generate()).await;
    let fleet = Fleet::new(
        endpoint.clone(),
        dir.path(),
        "Studio",
        offers(&dir, &endpoint, "Studio"),
    )
    .await;
    // The endpoint is closed and the mesh never opened: whatever the call
    // reaches is in this process.
    endpoint.close().await;
    let started = Instant::now();
    let client = fleet.client(fleet.id()).await.expect("this Mac is local");
    let (_tx, mut rx) = client.open("status/1").await.expect("the node opens it");
    let Some(Report::Offers(offers)) = rx.recv().await.expect("the node answers") else {
        panic!("the node's first report is its offers");
    };
    assert!(!offers.is_empty());
    assert!(started.elapsed() < DEADLINE, "{:?}", started.elapsed());
}

#[tokio::test]
async fn a_local_file_read_does_not_wait_for_the_services() {
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&SecretKey::generate()).await;
    // `run` never starts: the blob store and sync are not up.
    let fleet = Fleet::new(
        endpoint.clone(),
        dir.path(),
        "Studio",
        offers(&dir, &endpoint, "Studio"),
    )
    .await;
    let client = fleet.client(fleet.id()).await.expect("this Mac is local");
    let (mut tx, mut rx) = client.open(files::PROTOCOL).await.expect("opens");
    tx.send(&files::Request::Roots {}).await.expect("sends");
    let reply = timeout(DEADLINE, rx.recv::<files::Reply>())
        .await
        .expect("the node answers without waiting for the services")
        .expect("the stream reads");
    assert!(
        matches!(reply, Some(files::Reply::Roots { .. })),
        "{reply:?}"
    );
}

#[tokio::test]
async fn a_machine_that_is_off_fails_within_three_seconds() {
    let mac = start("Studio").await;
    let host = mac.fleet.mesh().expect("the mesh is open");
    let (mesh, router, endpoint) = join(&mac.fleet, DeviceKind::Node, "Server").await;
    let id = endpoint.id();
    until(&host, |mesh| online(mesh, id)).await;
    // Switched off: its endpoint closes, and with it the link.
    router.shutdown().await.expect("the router stops");
    drop(mesh);
    endpoint.close().await;
    until(&host, |mesh| !online(mesh, id)).await;
    let started = Instant::now();
    let error = mac.fleet.client(id).await.expect_err("the machine is off");
    assert!(
        started.elapsed() < DEADLINE + Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(error.to_string(), "Server is offline");
}

#[tokio::test]
async fn a_second_desktop_joins_and_sees_the_machines() {
    let first = start("Studio").await;
    let ticket = first
        .fleet
        .mesh()
        .expect("the mesh is open")
        .invite(DeviceKind::Desktop, Duration::from_secs(3600))
        .await
        .expect("a ticket is minted");
    // The second Mac joined before it started, then starts from that.
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&SecretKey::generate()).await;
    let joined = Mesh::join(dir.path(), endpoint.clone(), &ticket, "Laptop")
        .await
        .expect("the desktop joins");
    drop(joined);
    let second = Fleet::new(
        endpoint.clone(),
        dir.path(),
        "Laptop",
        offers(&dir, &endpoint, "Laptop"),
    )
    .await;
    second.run().await;
    let theirs = second.mesh().expect("the joined mesh opens");
    until(&theirs, |mesh| online(mesh, first.fleet.id())).await;
    let mut machines: Vec<_> = second
        .machines()
        .into_iter()
        .map(|status| (status.device.name, status.device.kind))
        .collect();
    machines.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        machines,
        [
            ("Laptop".to_owned(), DeviceKind::Desktop),
            ("Studio".to_owned(), DeviceKind::Desktop)
        ]
    );
    // And it reaches the first Mac's node over the mesh.
    let client = second.client(first.fleet.id()).await.expect("connected");
    let (_tx, mut rx) = client.open("status/1").await.expect("the node opens it");
    let report = rx.recv::<Report>().await.expect("the node answers");
    assert!(matches!(report, Some(Report::Offers(_))), "{report:?}");
}

#[tokio::test]
async fn a_machine_joining_wakes_what_follows_the_mesh() {
    let mac = start("Studio").await;
    let mut changes = mac.fleet.changes();
    let (_mesh, _router, _node) = join(&mac.fleet, DeviceKind::Node, "Server").await;

    let woke = timeout(LIMIT, changes.changed()).await;
    assert_eq!(woke.ok(), Some(true));
}

/// Issue #408: a session moving here from a node waited forever, since
/// its source could not ask this Mac how the move stood.
#[tokio::test]
async fn a_node_reaches_the_session_reads_of_this_mac() {
    let mac = start("Studio").await;
    let host = mac.fleet.mesh().expect("the mesh is open");
    let (_mesh, _router, node) = join(&mac.fleet, DeviceKind::Node, "Server").await;
    until(&host, |mesh| online(mesh, node.id())).await;
    let connection = timeout(
        LIMIT,
        node.connect(mac.endpoint.addr(), beep_node::agents::SESSIONS_ALPN),
    )
    .await
    .expect("connects in time")
    .expect("this Mac serves beep-sessions/1");
    assert_eq!(connection.alpn(), beep_node::agents::SESSIONS_ALPN);
}

/// Issue #390: a node runs commands on this Mac while Let Beep use this
/// Mac is on, and is told so plainly, not "offline", while it is off.
#[tokio::test]
async fn a_node_runs_commands_on_this_mac_only_while_it_is_lent() {
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&SecretKey::generate()).await;
    let mac_offers = offers(&dir, &endpoint, "Studio");
    let fleet =
        Arc::new(Fleet::new(endpoint.clone(), dir.path(), "Studio", mac_offers.clone()).await);
    fleet.run().await;
    let host = fleet.mesh().expect("the mesh is open");
    let (mesh, _router, node) = join(&fleet, DeviceKind::Node, "Server").await;
    until(&host, |mesh| online(mesh, node.id())).await;
    until(&mesh, |mesh| online(mesh, fleet.id())).await;
    let client = with_deadline(fleet.id(), &mesh)
        .await
        .expect("the node reaches this Mac");
    let request = Request {
        session: SessionId::from("a3f9c1").to_owned(),
        tool: Tool::RunOn,
        args: r#"{"command":"echo ran-here"}"#.to_owned(),
        approved: Default::default(),
    };
    let Reply::Done { text, failed } = client
        .call(tools::PROTOCOL, &request)
        .await
        .expect("the node answers")
    else {
        panic!("the call was not answered");
    };
    assert!(!failed && text.contains("ran-here"), "{text}");
    mac_offers.lend_to_peers(false);
    let Reply::Done { text, failed } = client
        .call(tools::PROTOCOL, &request)
        .await
        .expect("the node answers")
    else {
        panic!("the call was not answered");
    };
    assert!(failed, "{text}");
    assert_eq!(
        text,
        "Studio doesn't run commands: Let Beep use this computer is off in its Settings."
    );
}

async fn ticket(mac: &Mac) -> JoinTicket {
    mac.fleet
        .mesh()
        .expect("the mesh is open")
        .invite(DeviceKind::Desktop, Duration::from_secs(3600))
        .await
        .expect("a ticket is minted")
}

#[tokio::test]
async fn a_lone_mac_joins_another_mesh_and_keeps_its_settings() {
    let studio = start("Studio").await;
    let ticket = ticket(&studio).await;
    let laptop = start("Laptop").await;
    let key = Key::Project("notes".to_owned());
    laptop
        .fleet
        .mesh()
        .expect("the mesh is open")
        .set(key.clone(), Some(b"keep".to_vec()))
        .await
        .expect("the setting is written");
    laptop.fleet.join(&ticket, "Laptop").await.expect("joins");
    let mine = laptop.fleet.mesh().expect("the new mesh is open");
    assert_eq!(mine.devices().len(), 2);
    assert_eq!(mine.get(&key), Some(b"keep".to_vec()));
    let theirs = studio.fleet.mesh().expect("the mesh is open");
    until(&theirs, |mesh| mesh.get(&key).is_some()).await;
    assert_eq!(theirs.get(&key), Some(b"keep".to_vec()));
    // And the new mesh answers: the first Mac reaches the node.
    until(&theirs, |mesh| online(mesh, laptop.fleet.id())).await;
    let client = studio
        .fleet
        .client(laptop.fleet.id())
        .await
        .expect("connected");
    let (_tx, mut rx) = client.open("status/1").await.expect("the node opens it");
    let report = rx.recv::<Report>().await.expect("the node answers");
    assert!(matches!(report, Some(Report::Offers(_))), "{report:?}");
}

#[tokio::test]
async fn this_mac_cannot_remove_itself() {
    let mac = start("Studio").await;
    let error = mac
        .fleet
        .remove(mac.fleet.id())
        .await
        .expect_err("it stays");
    assert_eq!(error.to_string(), "this computer cannot remove itself");
    assert_eq!(mac.fleet.machines().len(), 1);
}

#[tokio::test]
async fn a_mac_with_other_devices_does_not_leave_its_mesh() {
    let studio = start("Studio").await;
    let laptop = start("Laptop").await;
    laptop
        .fleet
        .join(&ticket(&studio).await, "Laptop")
        .await
        .expect("joins");
    let other = start("Other").await;
    let error = laptop
        .fleet
        .join(&ticket(&other).await, "Laptop")
        .await
        .expect_err("it has company");
    assert_eq!(
        error.to_string(),
        "this computer is in a mesh with other devices"
    );
    assert_eq!(laptop.fleet.machines().len(), 2);
}

#[tokio::test]
async fn a_local_sync_call_waits_for_the_services_and_a_shell_does_not() {
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&SecretKey::generate()).await;
    let fleet = Fleet::new(
        endpoint.clone(),
        dir.path(),
        "Studio",
        offers(&dir, &endpoint, "Studio"),
    )
    .await;
    fleet.serve().await;
    let client = fleet.client(fleet.id()).await.expect("this Mac is local");
    timeout(LIMIT, client.open("shell/1"))
        .await
        .expect("answered in time")
        .expect("a shell opens at once");
    let waiting = client.clone();
    let sync = tokio::spawn(async move { waiting.open("sync/1").await.map(|_| ()) });

    for _ in 0..50 {
        tokio::task::yield_now().await;
    }

    assert!(!sync.is_finished(), "sync was served before its service");
    fleet.start_services().await;
    timeout(LIMIT, sync)
        .await
        .expect("answered in time")
        .expect("the task ran")
        .expect("sync opens once its service is up");
}

#[tokio::test]
async fn a_mesh_that_will_not_open_is_not_an_offline_machine() {
    let dir = TempDir::new().expect("a temp dir");
    std::fs::create_dir_all(dir.path().join("mesh2")).expect("a mesh folder");
    std::fs::write(
        dir.path().join("mesh2").join("devices.bin"),
        b"not a device list",
    )
    .expect("a broken list");
    let endpoint = bind(&SecretKey::generate()).await;
    let fleet = Fleet::new(
        endpoint.clone(),
        dir.path(),
        "Studio",
        offers(&dir, &endpoint, "Studio"),
    )
    .await;
    fleet.run().await;
    let started = Instant::now();
    let error = fleet
        .client(SecretKey::generate().public())
        .await
        .expect_err("there is no mesh");
    assert!(matches!(error, DesktopError::NoMesh(_)), "{error:?}");
    assert!(started.elapsed() < DEADLINE);
    assert!(fleet.machines().is_empty());
}

#[tokio::test]
async fn a_join_does_not_swap_a_mesh_that_is_still_in_use() {
    let studio = start("Studio").await;
    let ticket = ticket(&studio).await;
    let laptop = start("Laptop").await;
    let held = laptop.fleet.mesh().expect("the mesh is open");
    let error = laptop
        .fleet
        .join(&ticket, "Laptop")
        .await
        .expect_err("another handle is alive");
    assert_eq!(error.to_string(), "the mesh is still in use");
    // Nothing changed: the same mesh still answers, and joining works
    // once the other handle is gone.
    assert_eq!(laptop.fleet.machines().len(), 1);
    assert!(!laptop.dir.path().join("carry.json").exists());
    drop(held);
    laptop.fleet.join(&ticket, "Laptop").await.expect("joins");
    assert_eq!(laptop.fleet.machines().len(), 2);
    assert!(!laptop.dir.path().join("carry.json").exists());
}

#[tokio::test]
async fn a_join_waits_for_a_handle_that_is_about_to_drop() {
    let studio = start("Studio").await;
    let ticket = ticket(&studio).await;
    let laptop = start("Laptop").await;
    let held = laptop.fleet.mesh().expect("the mesh is open");
    // A call that ends a moment after the join began.
    tokio::spawn(async move {
        tokio::task::yield_now().await;
        drop(held);
    });
    laptop.fleet.join(&ticket, "Laptop").await.expect("joins");
    assert_eq!(laptop.fleet.machines().len(), 2);
}

#[tokio::test]
async fn settings_a_join_was_carrying_are_set_at_the_next_start() {
    let dir = TempDir::new().expect("a temp dir");
    carry::save(dir.path(), &[(Key::Flsh, b"keep".to_vec())]).expect("saved");
    let endpoint = bind(&SecretKey::generate()).await;
    let fleet = Fleet::new(
        endpoint.clone(),
        dir.path(),
        "Studio",
        offers(&dir, &endpoint, "Studio"),
    )
    .await;
    fleet.run().await;
    let mesh = fleet.mesh().expect("the mesh is open");
    assert_eq!(mesh.get(&Key::Flsh), Some(b"keep".to_vec()));
    assert!(!dir.path().join("carry.json").exists());
}

#[tokio::test]
async fn the_node_follows_the_new_mesh_after_a_join() {
    let studio = start("Studio").await;
    let ticket = ticket(&studio).await;
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&SecretKey::generate()).await;
    let node_offers = offers(&dir, &endpoint, "Laptop");
    let laptop = Fleet::new(endpoint, dir.path(), "Laptop", node_offers.clone()).await;
    laptop.run().await;
    laptop.join(&ticket, "Laptop").await.expect("joins");
    // A setting the first Mac writes in the mesh the Laptop joined
    // reaches the Laptop's node, which followed its old mesh before.
    let setting = HarnessConfig {
        enabled: false,
        ..HarnessConfig::new("codex")
    };
    studio
        .fleet
        .mesh()
        .expect("the mesh is open")
        .set(
            Key::Harness("codex".to_owned()),
            Some(setting.to_bytes().expect("it encodes")),
        )
        .await
        .expect("the setting is written");
    timeout(LIMIT, async {
        while node_offers.agents.catalog.settings().harnesses != [setting.clone()] {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the node took the setting from the new mesh");
}

/// A command peer that lists one session, or never answers.
#[derive(Debug)]
struct SessionsProtocol(bool);

impl iroh::protocol::ProtocolHandler for SessionsProtocol {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        use beep_proto::node::{
            Opened,
            agent::{SessionInfo, SessionsReply, SessionsRequest},
            commands::Open,
            link,
        };
        let mut streams = JoinSet::new();
        loop {
            tokio::select! {
                stream = connection.accept_bi() => {
                    let Ok((send, recv)) = stream else { return Ok(()); };
                    let answers = self.0;
                    streams.spawn(async move {
                        let (mut tx, mut rx) = link(send, recv);
                        let Ok(Some(Open::Session { protocol, .. })) = rx.recv::<Open>().await else { return; };
                        if protocol != "agents/1" {
                            let _ = tx.send(&Opened::Refused { reason: "not offered".into() }).await;
                            return;
                        }
                        if tx.send(&Opened::Ready {}).await.is_err() { return; }
                        let Ok(Some(_)) = rx.recv::<SessionsRequest>().await else { return; };
                        if answers {
                            let _ = tx.send(&SessionsReply::Sessions { sessions: vec![SessionInfo { id: "s1".into(), agent: "claude".into(), ..SessionInfo::default() }] }).await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    });
                },
                Some(result) = streams.join_next() => result.unwrap(),
            }
        }
    }
}

#[tokio::test]
async fn one_peer_that_never_answers_does_not_hold_the_others() {
    let mac = start("Studio").await;
    let host = mac.fleet.mesh().expect("the mesh is open");
    let (_quick, _quick_router, quick_endpoint) = join_serving(
        &mac.fleet,
        DeviceKind::Node,
        "Quick",
        Some(SessionsProtocol(true)),
    )
    .await;
    let (_slow, _slow_router, slow_endpoint) = join_serving(
        &mac.fleet,
        DeviceKind::Node,
        "Slow",
        Some(SessionsProtocol(false)),
    )
    .await;
    let (quick_id, slow_id) = (quick_endpoint.id(), slow_endpoint.id());
    until(&host, |mesh| {
        online(mesh, quick_id) && online(mesh, slow_id)
    })
    .await;
    let started = Instant::now();
    let (quick, slow) = tokio::join!(
        agents::sessions_of(&mac.fleet, quick_id),
        agents::sessions_of(&mac.fleet, slow_id),
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    let quick = quick.expect("the quick peer answers");
    assert_eq!(quick.len(), 1, "{quick:?}");
    assert_eq!(quick[0].session.machine_name, "Quick");
    assert!(slow.is_none());
}

#[tokio::test]
async fn a_desktop_clears_an_observer_marker_carried_from_an_older_build() {
    let mac = start("build-box").await;
    let key = Key::Observer(mac.fleet.id().to_string());
    carry::save(mac.dir.path(), &[(key, b"desktop".to_vec())])
        .expect("the previous controller's settings are saved");
    mac.fleet.stop().await;
    let again = Fleet::new(
        mac.endpoint.clone(),
        mac.dir.path(),
        "build-box",
        offers(&mac.dir, &mac.endpoint, "build-box"),
    )
    .await;
    again.run().await;
    let mesh = again
        .ready()
        .await
        .expect("the full build reopens the mesh");
    assert!(
        !mesh.is_observer(again.id()),
        "a full build advertises local hosting"
    );
    again.stop().await;
}
