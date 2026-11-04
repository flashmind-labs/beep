//! The machines this Mac reaches itself, with no server in between: the
//! mesh it belongs to, and the node in this process. A call to this Mac
//! never touches the network; a call to another machine goes over its
//! mesh connection.
//!
//! The mesh opens, or is made on first launch with this Mac as its first
//! device, off the main thread. Nothing the first paint needs waits on it.

mod mesh;

use crate::error::{DesktopError, Result};
use beep_mesh::{DEADLINE, DeviceStatus, Mesh, PeerClient, SessionFn, with_deadline};
use beep_node::link::{Offers, peers::Peers};
use beep_proto::client::PushHandler;
use iroh::{Endpoint, EndpointId};
use mesh::Serving;
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};
use tokio::{sync::watch, time::timeout};

pub struct Fleet {
    me: EndpointId,
    endpoint: Endpoint,
    /// The embedded node's folder, which holds the mesh.
    dir: PathBuf,
    name: String,
    peers: Peers,
    /// The embedded node, as a call to this Mac reaches it.
    session: SessionFn,
    mesh: watch::Sender<Option<Mesh>>,
    serving: Mutex<Option<Serving>>,
}

impl Fleet {
    /// The fleet of the node in `dir` that serves `offers`, on `endpoint`.
    /// Does nothing slow: calls to this Mac work at once, and `run` opens
    /// the mesh.
    pub fn new(endpoint: Endpoint, dir: &Path, name: &str, offers: Offers) -> Self {
        let me = endpoint.id();
        let peers = Peers::new(endpoint.clone(), dir.to_owned(), name.to_owned(), offers);
        Self {
            me,
            session: peers.local_session(),
            endpoint,
            dir: dir.to_owned(),
            name: name.to_owned(),
            peers,
            mesh: watch::channel(None).0,
            serving: Mutex::default(),
        }
    }

    /// Opens the mesh, or makes it, then answers peers on the endpoint, and
    /// `push` for a server's notices, until `stop`. Takes the time it takes
    /// and is meant to run in the background.
    pub async fn run(&self, push: Option<PushHandler>) {
        match mesh::open_or_create(&self.dir, &self.endpoint, &self.name).await {
            Ok(mesh) => {
                // The node reads its own kind from the mesh's device list.
                self.peers.install(mesh.clone());
                self.mesh.send_replace(Some(mesh));
            }
            Err(e) => log::warn!("not in a mesh: {e:#}"),
        }
        let serving = Serving::start(&self.endpoint, &self.dir, &self.peers, push).await;
        *self.serving.lock().unwrap_or_else(PoisonError::into_inner) = Some(serving);
    }

    /// Stops answering peers and saves what the node's services hold.
    pub async fn stop(&self) {
        let serving = self
            .serving
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(serving) = serving {
            serving.stop().await;
        }
    }

    pub fn id(&self) -> EndpointId {
        self.me
    }

    /// The mesh, once it is open.
    pub fn mesh(&self) -> Option<Mesh> {
        self.mesh.borrow().clone()
    }

    /// The mesh, waiting for `run` to open it.
    pub async fn ready(&self) -> Option<Mesh> {
        let mut mesh = self.mesh.subscribe();
        let ready = mesh.wait_for(Option::is_some).await.ok()?;
        ready.clone()
    }

    /// The machines of the mesh, each with whether it is online. Empty until
    /// the mesh is open.
    pub fn machines(&self) -> Vec<DeviceStatus> {
        self.mesh().map(|mesh| mesh.devices()).unwrap_or_default()
    }

    /// A client for `machine`. This Mac's own node is served in this process
    /// at once, with no network and no waiting. Another member gets
    /// `DEADLINE` to be connected, then is offline.
    pub async fn client(&self, machine: EndpointId) -> Result<PeerClient> {
        if machine == self.me {
            return Ok(PeerClient::local(self.session.clone()));
        }
        let connect = async {
            let mesh = self.ready().await?;
            with_deadline(machine, &mesh).await.ok()
        };
        match timeout(DEADLINE, connect).await {
            Ok(Some(client)) => Ok(client),
            Ok(None) | Err(_) => Err(DesktopError::Offline(self.name_of(machine))),
        }
    }

    /// What the mesh calls `machine`, or its short id.
    fn name_of(&self, machine: EndpointId) -> String {
        self.machines()
            .into_iter()
            .find(|status| status.device.id == machine)
            .map_or_else(
                || machine.fmt_short().to_string(),
                |status| status.device.name,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_mesh::{DeviceKind, JOIN_ALPN, Mesh, PEER_ALPN, transport_config};
    use beep_node::machine::provision::Provision;
    use beep_proto::node::{
        status::Report,
        tools::{self, Reply, Request, Tool},
    };
    use iroh::{SecretKey, endpoint::presets, protocol::Router};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    const LIMIT: Duration = Duration::from_secs(60);

    /// An endpoint on loopback with no relay and no discovery: nothing here
    /// can reach a network.
    async fn bind(key: &SecretKey) -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .secret_key(key.clone())
            .transport_config(transport_config())
            .bind()
            .await
            .expect("the endpoint binds")
    }

    fn offers(dir: &TempDir) -> Offers {
        Offers::local(&[], Provision::new(dir.path()))
    }

    struct Mac {
        fleet: Arc<Fleet>,
        endpoint: Endpoint,
        dir: TempDir,
    }

    /// A Mac that has started, with its mesh open and serving.
    async fn start(name: &str) -> Mac {
        let dir = TempDir::new().expect("a temp dir");
        let endpoint = bind(&SecretKey::generate()).await;
        let fleet = Arc::new(Fleet::new(endpoint.clone(), dir.path(), name, offers(&dir)));
        fleet.run(None).await;
        Mac {
            fleet,
            endpoint,
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
                updates.changed().await.expect("the mesh is alive");
            }
        })
        .await
        .expect("the condition came true in time");
    }

    fn online(mesh: &Mesh, id: EndpointId) -> bool {
        mesh.devices().iter().any(|d| d.device.id == id && d.online)
    }

    /// A device that joins the mesh of `host` as `kind`, serving its own
    /// side of the mesh so the two connect.
    async fn join(host: &Fleet, kind: DeviceKind, name: &str) -> (Mesh, Router, Endpoint) {
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
        let router = Router::builder(endpoint.clone())
            .accept(PEER_ALPN, peer)
            .accept(JOIN_ALPN, join)
            .spawn();
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
        assert!(mac.dir.path().join("mesh").join("devices.bin").exists());
        // The next launch opens the same mesh, not a new one.
        mac.fleet.stop().await;
        let again = Fleet::new(
            mac.endpoint.clone(),
            mac.dir.path(),
            "Studio",
            offers(&mac.dir),
        );
        again.run(None).await;
        assert_eq!(again.machines().len(), 1);
        again.stop().await;
    }

    #[tokio::test]
    async fn a_call_to_this_mac_does_not_touch_the_network() {
        let dir = TempDir::new().expect("a temp dir");
        let endpoint = bind(&SecretKey::generate()).await;
        let fleet = Fleet::new(endpoint.clone(), dir.path(), "Studio", offers(&dir));
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
        let second = Fleet::new(endpoint, dir.path(), "Laptop", offers(&dir));
        second.run(None).await;
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
    async fn a_node_may_not_run_commands_on_this_mac() {
        let mac = start("Studio").await;
        let host = mac.fleet.mesh().expect("the mesh is open");
        let (mesh, _router, endpoint) = join(&mac.fleet, DeviceKind::Node, "Server").await;
        until(&host, |mesh| online(mesh, endpoint.id())).await;
        until(&mesh, |mesh| online(mesh, mac.fleet.id())).await;
        let client = with_deadline(mac.fleet.id(), &mesh)
            .await
            .expect("the node reaches this Mac");
        let request = Request {
            session: "a3f9c1".to_owned(),
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
        assert!(failed && text.contains("desktop"), "{text}");
        assert!(!text.contains("ran-here"));
    }
}
