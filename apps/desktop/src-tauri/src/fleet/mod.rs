//! The machines this Mac reaches itself, with no server in between: the
//! mesh it belongs to, and the node in this process. A call to this Mac
//! never touches the network; a call to another machine goes over its
//! mesh connection.
//!
//! The mesh opens, or is made on first launch with this Mac as its first
//! device, off the main thread. Nothing the first paint needs waits on it.

pub(crate) mod agents;
pub(crate) mod alerts;
mod carry;
pub(crate) mod files;
mod mesh;
pub(crate) mod migrate;
pub(crate) mod projects;
mod status;
pub(crate) mod terminals;

use crate::{
    beep::AsyncMutex,
    error::{DesktopError, Result},
};
use beep_mesh::{
    DEFAULT_TICKET_TTL, DeviceKind, DeviceStatus, InUse, JoinTicket, Mesh, PeerClient, SessionFn,
    machine::releases::ToolReleases, with_deadline,
};
use beep_node::{
    files::Folders,
    link::{Offers, peers::Peers},
};
use beep_proto::client::PushHandler;
use iroh::{Endpoint, EndpointId};
use mesh::Serving;
pub use status::Status;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tauri::State;
use tauri::async_runtime::JoinHandle;
use tokio::{
    sync::watch,
    time::{Instant, sleep, timeout},
};

/// The machines of the mesh, as a command takes them.
pub(crate) type Machines<'a> = State<'a, Arc<Fleet>>;

/// What a server adds to a list of the mesh's machines: nothing when there
/// is no server or it cannot answer.
pub(crate) fn extra<T>(result: Result<Vec<T>>) -> Vec<T> {
    match result {
        Ok(rows) => rows,
        Err(DesktopError::NoServer | DesktopError::Unlinked | DesktopError::Revoked) => Vec::new(),
        Err(e) => {
            log::warn!("the server's list was left out: {e}");
            Vec::new()
        }
    }
}

/// How long a call to another machine waits for the mesh to open.
const OPENING: Duration = Duration::from_secs(10);

/// How long a join waits for the last handles to the old mesh to drop: calls
/// that were running when it began.
#[cfg(not(test))]
const CLOSE_WAIT: Duration = Duration::from_secs(5);
#[cfg(test)]
const CLOSE_WAIT: Duration = Duration::from_millis(300);

/// How often a join looks again while it waits.
const CLOSE_POLL: Duration = Duration::from_millis(25);

/// Closes `mesh`, trying again while another handle is alive, for at most
/// `wait`. Handed back, still running, when one stays.
async fn close_within(mut mesh: Mesh, wait: Duration) -> Result<(), InUse> {
    let deadline = Instant::now() + wait;
    loop {
        match mesh.close().await {
            Ok(()) => return Ok(()),
            Err(in_use) if Instant::now() >= deadline => return Err(in_use),
            Err(InUse(back)) => {
                mesh = back;
                sleep(CLOSE_POLL).await;
            }
        }
    }
}

/// Where the mesh stands.
#[derive(Clone)]
enum Opened {
    Opening,
    Open(Mesh),
    /// It could not be opened or made; the text says why.
    Failed(String),
}

pub struct Fleet {
    me: EndpointId,
    endpoint: Endpoint,
    /// The embedded node's folder, which holds the mesh.
    dir: PathBuf,
    name: String,
    peers: Peers,
    /// What the user's home shares, for files and sync.
    folders: Folders,
    /// The embedded node, as a call to this Mac reaches it.
    session: SessionFn,
    mesh: watch::Sender<Opened>,
    status: Status,
    serving: Mutex<Option<Arc<Serving>>>,
    /// One `join` at a time.
    joining: AsyncMutex<()>,
    /// The account move running now, which holds the mesh.
    move_task: Mutex<Option<JoinHandle<()>>>,
    /// The newest release of each tool a node installs.
    releases: ToolReleases,
}

impl Fleet {
    /// The fleet of the node in `dir` that serves `offers`, on `endpoint`.
    /// Does nothing slow: calls to this Mac work at once, and `run` opens
    /// the mesh.
    pub fn new(endpoint: Endpoint, dir: &Path, name: &str, offers: Offers) -> Self {
        let me = endpoint.id();
        let peers = Peers::new(endpoint.clone(), dir.to_owned(), name.to_owned(), offers);
        // Sessions that open these wait for the services `run` starts.
        peers.expect(&mesh::LATER);
        // Local file reads need no blob store: served from the first call.
        let folders = mesh::home_folders();
        if !folders.is_empty() {
            peers.offer(Arc::new(folders.clone()));
        }
        let session = peers.local_session();
        Self {
            me,
            status: Status::new(me, session.clone()),
            session,
            endpoint,
            dir: dir.to_owned(),
            name: name.to_owned(),
            peers,
            folders,
            mesh: watch::channel(Opened::Opening).0,
            serving: Mutex::default(),
            joining: AsyncMutex::new(()),
            move_task: Mutex::default(),
            releases: ToolReleases::default(),
        }
    }

    /// `serve`, then `start_services`. Takes the time it takes and is meant
    /// to run in the background.
    pub async fn run(&self, push: Option<PushHandler>) {
        self.serve(push).await;
        self.start_services().await;
    }

    /// Answers peers on the endpoint, and `push` for a server's notices,
    /// and opens the mesh. Quick: it waits for no service.
    async fn serve(&self, push: Option<PushHandler>) {
        let serving = Arc::new(Serving::spawn(&self.endpoint, &self.peers, push));
        *self.serving.lock().unwrap_or_else(PoisonError::into_inner) = Some(serving);
        self.open().await;
    }

    /// Starts the transfer and sync services, which take longer. Until they
    /// are up, sessions that open their protocols wait.
    async fn start_services(&self) {
        let serving = self
            .serving
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match serving {
            Some(serving) => {
                serving
                    .start_services(&self.endpoint, &self.dir, &self.peers, self.folders.clone())
                    .await;
            }
            None => self.peers.settle(),
        }
    }

    /// Opens the mesh, or makes it.
    async fn open(&self) {
        match mesh::open_or_create(&self.dir, &self.endpoint, &self.name).await {
            Ok(mesh) => {
                // The node reads its own kind from the mesh's device list.
                self.peers.install(mesh.clone());
                // Settings a join was carrying when the app stopped.
                if let Err(e) = carry::replay(&self.dir, &mesh).await {
                    log::warn!("{e}");
                }
                self.status.watch(mesh.clone()).await;
                self.mesh.send_replace(Opened::Open(mesh));
            }
            Err(e) => {
                log::warn!("not in a mesh: {e:#}");
                self.mesh.send_replace(Opened::Failed(e.to_string()));
            }
        }
    }

    /// Keeps `task`, the account move, as the one running; a move before it is cancelled.
    pub(crate) fn moving(&self, task: JoinHandle<()>) {
        let before = self
            .move_task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(task);
        if let Some(before) = before {
            before.abort();
        }
    }

    /// Cancels the account move and waits until it lets go of the mesh. It starts again with
    /// the next launch, or the next sign-in or join.
    async fn stop_move(&self) {
        let task = self
            .move_task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            task.abort();
            // Cancelled or finished: either way its mesh handle is gone.
            let _ = task.await;
        }
    }

    /// Joins the mesh `ticket` names, as `name`. A Mac that made its own mesh
    /// on first launch leaves it while it is the mesh's only device, and
    /// brings its settings along: they are signed for the old mesh, so they
    /// are written again in the new one. A Mac with other devices in its
    /// mesh does not leave it, and neither does one whose mesh stays in use
    /// elsewhere in the app past `CLOSE_WAIT`.
    pub async fn join(&self, ticket: &JoinTicket, name: &str) -> Result<()> {
        let _one = self.joining.lock().await;
        let old = self.ready().await?;
        if old.devices().len() > 1 {
            return Err(DesktopError::BadRequest(
                "this Mac is in a mesh with other devices".into(),
            ));
        }
        // The move holds the mesh while it reads the server, and a mesh in use cannot close.
        self.stop_move().await;
        // On disk first, so a crash after the swap still brings them along.
        carry::save(&self.dir, &old.scan(""))?;
        self.peers.leave().await;
        self.mesh.send_replace(Opened::Opening);
        // The status follows and the alert follows (which let go on seeing the
        // mesh close) stop holding it; calls still running get `CLOSE_WAIT`.
        self.status.stop().await;
        if let Err(in_use) = close_within(old, CLOSE_WAIT).await {
            let message = in_use.to_string();
            let InUse(old) = in_use;
            self.peers.install(old.clone());
            self.status.watch(old.clone()).await;
            self.mesh.send_replace(Opened::Open(old));
            carry::discard(&self.dir);
            return Err(DesktopError::BadRequest(message));
        }
        let joined =
            Mesh::join_replacing_lone(&self.dir, self.endpoint.clone(), ticket, name).await;
        let new = match joined {
            Ok(new) => new,
            Err(e) => {
                // Nothing changed on disk: the old mesh opens again.
                carry::discard(&self.dir);
                self.open().await;
                return Err(e.into());
            }
        };
        self.peers.install(new.clone());
        self.status.watch(new.clone()).await;
        let carried = carry::replay(&self.dir, &new).await;
        self.mesh.send_replace(Opened::Open(new));
        carried
    }

    /// Stops answering peers, then stops the sync and transfer services, which
    /// save their state.
    pub async fn stop(&self) {
        let serving = self
            .serving
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(serving) = serving {
            serving.stop().await;
        }
        self.status.stop().await;
    }

    /// The newest release of each tool a node installs.
    pub(crate) fn releases(&self) -> &ToolReleases {
        &self.releases
    }

    pub fn id(&self) -> EndpointId {
        self.me
    }

    /// The mesh, once it is open.
    pub fn mesh(&self) -> Option<Mesh> {
        match &*self.mesh.borrow() {
            Opened::Open(mesh) => Some(mesh.clone()),
            _ => None,
        }
    }

    /// The mesh, waiting for it to open. Fails when it could not.
    pub async fn ready(&self) -> Result<Mesh> {
        let mut opened = self.mesh.subscribe();
        let opened = opened
            .wait_for(|opened| !matches!(opened, Opened::Opening))
            .await
            .map_err(|_| DesktopError::NoMesh("the app is closing".into()))?
            .clone();
        match opened {
            Opened::Open(mesh) => Ok(mesh),
            Opened::Failed(why) => Err(DesktopError::NoMesh(why)),
            Opened::Opening => Err(DesktopError::NoMesh("still opening".into())),
        }
    }

    /// The machines of the mesh, each with whether it is online. Empty until
    /// the mesh is open.
    pub fn machines(&self) -> Vec<DeviceStatus> {
        self.mesh().map(|mesh| mesh.devices()).unwrap_or_default()
    }

    /// What the machines of the mesh last reported.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Mints a ticket that brings a device of `kind` into the mesh. Only a
    /// desktop can, and this Mac is one.
    pub async fn invite(&self, kind: DeviceKind) -> Result<JoinTicket> {
        Ok(self.ready().await?.invite(kind, DEFAULT_TICKET_TTL).await?)
    }

    /// Removes `machine` from the mesh for good. Every other member drops it
    /// and its link.
    pub async fn remove(&self, machine: EndpointId) -> Result<()> {
        if machine == self.me {
            return Err(DesktopError::BadRequest(
                "this Mac cannot remove itself".into(),
            ));
        }
        Ok(self.ready().await?.remove(machine).await?)
    }

    /// A client for `machine`. This Mac's own node is served in this process
    /// at once, with no network and no waiting. For another member the mesh
    /// gets `OPENING` to be open, then the machine gets `DEADLINE` to be
    /// connected, then it is offline.
    pub async fn client(&self, machine: EndpointId) -> Result<PeerClient> {
        if machine == self.me {
            return Ok(PeerClient::local(self.session.clone()));
        }
        let mesh = timeout(OPENING, self.ready())
            .await
            .map_err(|_| DesktopError::NoMesh("still opening".into()))??;
        with_deadline(machine, &mesh)
            .await
            .map_err(|_| DesktopError::Offline(self.name_of(machine)))
    }

    /// `machine` as a member of the mesh, or this Mac. `None` for a machine
    /// the mesh does not hold, which only a server can reach.
    pub fn member(&self, machine: &str) -> Option<EndpointId> {
        let id: EndpointId = machine.parse().ok()?;
        (id == self.me || self.mesh()?.is_member(id)).then_some(id)
    }

    /// The members that are online now, this Mac among them.
    pub fn online(&self) -> Vec<EndpointId> {
        self.machines()
            .into_iter()
            .filter(|status| status.online || status.device.id == self.me)
            .map(|status| status.device.id)
            .collect()
    }

    /// What the mesh calls `machine`, or its short id.
    pub(crate) fn name_of(&self, machine: EndpointId) -> String {
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
    use beep_mesh::{DEADLINE, DeviceKind, JOIN_ALPN, Key, Mesh, PEER_ALPN, transport_config};
    use beep_node::machine::provision::Provision;
    use beep_proto::node::{
        files,
        harness::HarnessConfig,
        mesh_state::StateValue,
        status::Report,
        tools::{self, Reply, Request, Tool},
    };
    use beep_proto::{client::Node, wire::PUSH_ALPN};
    use iroh::{SecretKey, endpoint::presets, protocol::Router};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

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

    pub(super) fn offers(dir: &TempDir) -> Offers {
        Offers::local(&[], Provision::new(dir.path()), false)
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
        let fleet = Arc::new(Fleet::new(endpoint.clone(), dir.path(), name, offers(&dir)));
        fleet.run(None).await;
        Mac {
            fleet,
            endpoint,
            dir,
        }
    }

    pub(super) async fn until(mesh: &Mesh, ok: impl Fn(&Mesh) -> bool) {
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

    pub(super) fn online(mesh: &Mesh, id: EndpointId) -> bool {
        mesh.devices().iter().any(|d| d.device.id == id && d.online)
    }

    /// A device that joins the mesh of `host` as `kind`, serving its own
    /// side of the mesh so the two connect.
    pub(super) async fn join(
        host: &Fleet,
        kind: DeviceKind,
        name: &str,
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
    async fn a_local_file_read_does_not_wait_for_the_services() {
        let dir = TempDir::new().expect("a temp dir");
        let endpoint = bind(&SecretKey::generate()).await;
        // `run` never starts: the blob store and sync are not up.
        let fleet = Fleet::new(endpoint, dir.path(), "Studio", offers(&dir));
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
        assert_eq!(error.to_string(), "this Mac cannot remove itself");
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
            "this Mac is in a mesh with other devices"
        );
        assert_eq!(laptop.fleet.machines().len(), 2);
    }

    #[tokio::test]
    async fn a_push_is_accepted_while_the_services_are_still_starting() {
        let dir = TempDir::new().expect("a temp dir");
        let endpoint = bind(&SecretKey::generate()).await;
        let server = bind(&SecretKey::generate()).await;
        let (_node, push) = Node::on_endpoint(endpoint.clone(), server.addr(), Arc::new(|_| true));
        let fleet = Fleet::new(endpoint.clone(), dir.path(), "Studio", offers(&dir));
        // The services are held back: `start_services` is not called.
        fleet.serve(Some(push)).await;
        let connection = timeout(LIMIT, server.connect(endpoint.addr(), PUSH_ALPN))
            .await
            .expect("answered in time")
            .expect("the push connection is accepted");
        assert!(connection.close_reason().is_none());
    }

    #[tokio::test]
    async fn a_local_sync_call_waits_for_the_services_and_a_shell_does_not() {
        let dir = TempDir::new().expect("a temp dir");
        let endpoint = bind(&SecretKey::generate()).await;
        let fleet = Fleet::new(endpoint, dir.path(), "Studio", offers(&dir));
        fleet.serve(None).await;
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
        std::fs::create_dir_all(dir.path().join("mesh")).expect("a mesh folder");
        std::fs::write(
            dir.path().join("mesh").join("devices.bin"),
            b"not a device list",
        )
        .expect("a broken list");
        let endpoint = bind(&SecretKey::generate()).await;
        let fleet = Fleet::new(endpoint, dir.path(), "Studio", offers(&dir));
        fleet.run(None).await;
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
        let fleet = Fleet::new(endpoint, dir.path(), "Studio", offers(&dir));
        fleet.run(None).await;
        let mesh = fleet.mesh().expect("the mesh is open");
        assert_eq!(mesh.get(&Key::Flsh), Some(b"keep".to_vec()));
        assert!(!dir.path().join("carry.json").exists());
    }

    #[tokio::test]
    async fn the_node_follows_the_new_mesh_after_a_join() {
        let studio = start("Studio").await;
        let ticket = ticket(&studio).await;
        let dir = TempDir::new().expect("a temp dir");
        let node_offers = offers(&dir);
        let endpoint = bind(&SecretKey::generate()).await;
        let laptop = Fleet::new(endpoint, dir.path(), "Laptop", node_offers.clone());
        laptop.run(None).await;
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
            while node_offers.agents.settings().harnesses != [setting.clone()] {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the node took the setting from the new mesh");
    }

    /// A node that lists one session of its own, or never answers.
    fn sessions_handler(answers: bool) -> beep_mesh::StreamHandler {
        use beep_proto::node::{
            Open, Opened,
            agent::{SessionInfo, SessionsReply, SessionsRequest},
            link,
        };
        Arc::new(move |stream: beep_mesh::PeerStream| {
            Box::pin(async move {
                let (mut tx, mut rx) = link(stream.send, stream.recv);
                let Ok(Some(_)) = rx.recv::<Open>().await else {
                    return;
                };
                tx.send(&Opened::Ready {}).await.ok();
                let Ok(Some(_)) = rx.recv::<SessionsRequest>().await else {
                    return;
                };
                if answers {
                    let info = SessionInfo {
                        id: "s1".into(),
                        agent: "claude".into(),
                        ..SessionInfo::default()
                    };
                    let reply = SessionsReply::Sessions {
                        sessions: vec![info],
                    };
                    tx.send(&reply).await.ok();
                } else {
                    std::future::pending::<()>().await;
                }
            })
        })
    }

    #[tokio::test]
    async fn one_peer_that_never_answers_does_not_hold_the_others() {
        let mac = start("Studio").await;
        let host = mac.fleet.mesh().expect("the mesh is open");
        let (quick, _quick_router, quick_endpoint) =
            join(&mac.fleet, DeviceKind::Node, "Quick").await;
        let (slow, _slow_router, slow_endpoint) = join(&mac.fleet, DeviceKind::Node, "Slow").await;
        quick.protocols().0.on_stream(sessions_handler(true));
        slow.protocols().0.on_stream(sessions_handler(false));
        let (quick_id, slow_id) = (quick_endpoint.id(), slow_endpoint.id());
        until(&host, |mesh| {
            online(mesh, quick_id) && online(mesh, slow_id)
        })
        .await;
        let started = Instant::now();
        let found = agents::peer_sessions(&mac.fleet).await;
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].machine_name, "Quick");
    }
}
