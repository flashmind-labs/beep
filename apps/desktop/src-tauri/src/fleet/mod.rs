//! The mesh and its machines, with a local node on the desktop.
//! A hosted call to this Mac stays in process; other calls use the mesh.
//!
//! The mesh opens, or is made on first launch with this Mac as its first
//! device, off the main thread. Nothing the first paint needs waits on it.

pub(crate) mod agents;
pub(crate) mod alerts;
pub(crate) mod asked;
#[cfg(desktop)]
mod carry;
mod copies;
pub(crate) mod events;
pub(crate) mod files;
pub(crate) mod home_signal;
#[cfg(desktop)]
mod mesh;
pub(crate) mod node_actor;
pub(crate) mod outbox;
pub(crate) mod phone;
pub(crate) mod projects;
pub(crate) mod session_lists;
pub(crate) mod session_mirror;
mod status;
pub(crate) mod terminal_rows;
pub(crate) mod terminals;
pub(crate) mod tool_updates;
pub(crate) mod view_signal;
pub(crate) mod work_copy;

use crate::error::{DesktopError, Result};
use beep_mesh::{
    DEFAULT_TICKET_TTL, DeviceKind, DeviceStatus, InUse, Invitation, JoinTicket, Mesh, PeerClient,
    SessionFn, machine::releases::ToolReleases, with_deadline,
};
#[cfg(desktop)]
use beep_node::files::Folders;
#[cfg(desktop)]
use beep_node::{link::Bound, link::peers::Peers};
use beep_proto::watched;
use copies::Copies;
use iroh::{Endpoint, EndpointId, SecretKey, protocol::Router};
#[cfg(desktop)]
use mesh::Serving;
pub(crate) use node_actor::ToolPush;
pub(crate) use session_mirror::SessionParent;
pub use status::{ClockOff, Status};
#[cfg(mobile)]
use std::time::SystemTime;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tauri::State;
use tokio::{sync::watch, time::timeout};

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

/// The machines of the mesh, as a command takes them.
pub(crate) type Machines<'a> = State<'a, Arc<Fleet>>;

/// How long a call to another machine waits for the mesh to open.
const OPENING: Duration = Duration::from_secs(10);
/// How long a phone app may stay in the background before its links are
/// dialed again when it comes back. iOS suspends an app a few seconds after
/// it leaves the screen.
#[cfg(mobile)]
const AWAY: Duration = Duration::from_secs(10);

/// How long a join waits for the last handles to the old mesh to drop: calls
/// that were running when it began.
#[cfg(not(test))]
const CLOSE_WAIT: Duration = Duration::from_secs(5);
#[cfg(test)]
const CLOSE_WAIT: Duration = Duration::from_millis(300);

/// The longest name a machine can be given, in characters.
const MAX_MACHINE_NAME: usize = 64;

/// Closes `mesh` once the other handles are gone, waiting at most `wait`
/// for them. Handed back, still running, when one stays.
async fn close_within(mesh: Mesh, wait: Duration) -> Result<(), InUse> {
    let _ = timeout(wait, mesh.released()).await;

    mesh.close().await
}

/// Where the mesh stands.
#[derive(Clone)]
enum Opened {
    Opening,
    /// A phone with no mesh yet: it waits for a desktop's phone ticket.
    #[cfg(mobile)]
    Unpaired,
    Open(Mesh),
    /// It could not be opened or made; the text says why.
    Failed(String),
}

/// The mesh's own updates, from the mesh open now.
fn members(opened: &mut watch::Receiver<Opened>) -> Option<watch::Receiver<u64>> {
    let mesh = watched::read_and_update(opened, |opened| match opened {
        Opened::Open(mesh) => Some(mesh.clone()),
        _ => None,
    });

    mesh.map(|mesh| mesh.updates())
}

/// Follows the mesh opening or being replaced, its devices, state and who
/// is online, and the machines' reports where they change the machine list.
pub(crate) struct Changes {
    opened: watch::Receiver<Opened>,
    members: Option<watch::Receiver<u64>>,
    status: watch::Receiver<status::Revision>,
    machines: u64,
}

impl Changes {
    /// Waits for the next change; false once the app is closing.
    pub(crate) async fn changed(&mut self) -> bool {
        loop {
            tokio::select! {
                changed = self.opened.changed() => {
                    if changed.is_err() {
                        return false;
                    }

                    self.members = members(&mut self.opened);
                    return true;
                }
                changed = async {
                    match &mut self.members {
                        Some(members) => members.changed().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if changed.is_ok() {
                        return true;
                    }

                    // That mesh closed; the next one comes through `opened`.
                    self.members = None;
                }
                changed = self.status.changed() => {
                    if changed.is_err() {
                        return false;
                    }

                    let machines = watched::read_and_update(&mut self.status, |status| status.machines);

                    if machines != self.machines {
                        self.machines = machines;
                        return true;
                    }
                }
            }
        }
    }
}

pub struct Fleet {
    pub(crate) me: EndpointId,
    endpoint: Endpoint,
    /// The embedded node's folder, which holds the mesh.
    dir: PathBuf,
    #[cfg(desktop)]
    name: String,
    /// The embedded node's links. A phone is a mesh observer and runs no node.
    #[cfg(desktop)]
    peers: Peers,
    /// What the user's home shares, for files and sync.
    #[cfg(desktop)]
    folders: Folders,
    /// The embedded node, as a call to this Mac reaches it. None on a phone.
    session: Option<SessionFn>,
    mesh: watch::Sender<Opened>,
    status: Status,
    #[cfg(desktop)]
    serving: Mutex<Option<Arc<Serving>>>,
    /// A phone's answer to the mesh and join protocols, held while the mesh is in use.
    observing: Mutex<Option<Router>>,
    /// Since when the phone app is in the background, by the wall clock, which
    /// also runs while the phone sleeps.
    #[cfg(mobile)]
    away: Mutex<Option<SystemTime>>,
    /// One `join` at a time.
    joining: AsyncMutex<()>,
    /// What is known of this Mac's copies of the machines' project folders.
    copies: Copies,
    /// The newest release of each tool a node installs.
    releases: ToolReleases,
    snapshots: Arc<events::Snapshots>,
    projections: node_actor::Projections,
    node_ops: Arc<node_actor::NodeOps>,
}

impl Fleet {
    /// The fleet of the node in `dir` that serves `offers`, on `endpoint`.
    /// The offers come from `Offers::bound` with the same endpoint, dir and name.
    /// Does nothing slow: calls to this Mac work at once, and `run` opens
    /// the mesh.
    #[cfg(desktop)]
    pub fn new(endpoint: Endpoint, dir: &Path, name: &str, offers: Bound) -> Self {
        let me = endpoint.id();
        let peers = Peers::new(offers);
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
            status: Status::new(me, Some(session.clone()), dir),
            session: Some(session),
            endpoint,
            dir: dir.to_owned(),
            name: name.to_owned(),
            peers,
            folders,
            mesh: watch::channel(Opened::Opening).0,
            serving: Mutex::default(),
            observing: Mutex::default(),
            joining: AsyncMutex::new(()),
            copies: Copies::default(),
            releases: ToolReleases::default(),
            snapshots: Arc::new(events::Snapshots::default()),
            projections: node_actor::Projections::default(),
            node_ops: Arc::new(node_actor::NodeOps::default()),
        }
    }

    /// The fleet of a phone: a mesh observer with no node, no folders and nothing served beyond
    /// the mesh and join protocols. `run` opens the mesh if the phone is paired; `pair` joins one.
    #[cfg(mobile)]
    pub fn observer(endpoint: Endpoint, dir: &Path) -> Self {
        let me = endpoint.id();
        Self {
            me,
            status: Status::new(me, None, dir),
            session: None,
            endpoint,
            dir: dir.to_owned(),
            mesh: watch::channel(Opened::Opening).0,
            observing: Mutex::default(),
            away: Mutex::default(),
            joining: AsyncMutex::new(()),
            copies: Copies::default(),
            releases: ToolReleases::default(),
            snapshots: Arc::new(events::Snapshots::default()),
            projections: node_actor::Projections::default(),
            node_ops: Arc::new(node_actor::NodeOps::default()),
        }
    }

    /// `serve`, then `start_services`. Takes the time it takes and is meant
    /// to run in the background.
    #[cfg(desktop)]
    pub async fn run(&self) {
        self.serve().await;
        self.start_services().await;
    }

    /// Opens the mesh the phone is paired with, if any.
    #[cfg(mobile)]
    pub async fn run(&self) {
        self.open_observer().await;
    }

    /// Answers peers on the endpoint and opens the mesh. Quick: it waits
    /// for no service.
    #[cfg(desktop)]
    async fn serve(&self) {
        let serving = Arc::new(Serving::spawn(&self.endpoint, &self.peers));
        *self.serving.lock().unwrap_or_else(PoisonError::into_inner) = Some(serving);
        self.open().await;
    }

    /// Starts the transfer and sync services, which take longer. Until they
    /// are up, sessions that open their protocols wait.
    #[cfg(desktop)]
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
    #[cfg(desktop)]
    async fn open(&self) {
        match mesh::open_or_create(&self.dir, &self.endpoint, &self.name).await {
            Ok(mesh) => {
                // Replay first, then clear any observer marker retained by
                // older desktop builds that did not host a local node.
                if let Err(e) = carry::replay(&self.dir, &mesh).await {
                    log::warn!("{e}");
                }
                if mesh.is_member(self.me)
                    && let Err(error) = mesh.set_desktop_hosting(true).await
                {
                    self.mesh.send_replace(Opened::Failed(error.to_string()));
                    return;
                }
                self.peers.install(mesh.clone());

                self.status.watch(mesh.clone()).await;
                self.mesh.send_replace(Opened::Open(mesh));
            }
            Err(e) => {
                log::warn!("not in a mesh: {e:#}");
                self.mesh.send_replace(Opened::Failed(e.to_string()));
            }
        }
    }

    /// Joins the mesh `ticket` names, as `name`. A Mac that made its own mesh
    /// on first launch leaves it while it is the mesh's only device, and
    /// brings its settings along: they are signed for the old mesh, so they
    /// are written again in the new one. A Mac with other devices in its
    /// mesh does not leave it, and neither does one whose mesh stays in use
    /// elsewhere in the app past `CLOSE_WAIT`.
    #[cfg(desktop)]
    pub async fn join(&self, invite: impl Into<Invitation>, name: &str) -> Result<()> {
        let peers = &self.peers;
        let _one = self.joining.lock().await;
        // A removed desktop still needs its saved mesh so the join can replace it.
        let old = self.ready().await.or_else(|error| {
            self.mesh()
                .filter(|mesh| !mesh.is_member(self.me))
                .ok_or(error)
        })?;
        // A mesh that removed this Mac has nothing to keep.
        if old.is_member(old.me()) && old.devices().len() > 1 {
            return Err(DesktopError::BadRequest(
                "this computer is in a mesh with other devices".into(),
            ));
        }
        // On disk first, so a crash after the swap still brings them along.
        carry::save(&self.dir, &old.scan(""))?;
        peers.leave().await;
        self.mesh.send_replace(Opened::Opening);
        // The status follows and the alert follows (which let go on seeing the
        // mesh close) stop holding it; calls still running get `CLOSE_WAIT`.
        self.status.stop().await;

        if let Err(in_use) = close_within(old, CLOSE_WAIT).await {
            let message = in_use.to_string();
            let InUse(old) = in_use;
            peers.install(old.clone());
            self.status.watch(old.clone()).await;
            self.mesh.send_replace(Opened::Open(old));
            carry::discard(&self.dir);
            return Err(DesktopError::BadRequest(message));
        }

        let joined =
            Mesh::join_replacing_lone(&self.dir, self.endpoint.clone(), invite, name).await;
        let new = match joined {
            Ok(new) => new,
            Err(e) => {
                // Nothing changed on disk: the old mesh opens again.
                carry::discard(&self.dir);
                self.open().await;
                return Err(e.into());
            }
        };
        let carried = carry::replay(&self.dir, &new).await;
        if let Err(error) = new.set_desktop_hosting(true).await {
            self.mesh.send_replace(Opened::Failed(error.to_string()));
            return Err(error.into());
        }
        peers.install(new.clone());
        self.status.watch(new.clone()).await;
        self.mesh.send_replace(Opened::Open(new));
        carried
    }

    /// A phone does not join with a desktop's ticket.
    #[cfg(mobile)]
    pub async fn join(&self, _invite: impl Into<Invitation>, _name: &str) -> Result<()> {
        Err(DesktopError::BadRequest(
            "a phone pairs with a phone ticket".into(),
        ))
    }

    /// Stops answering peers, then stops the sync and transfer services, which
    /// save their state.
    pub async fn stop(&self) {
        #[cfg(desktop)]
        let serving = self
            .serving
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        #[cfg(desktop)]
        let serving = async {
            if let Some(serving) = serving {
                serving.stop().await;
            }
        };
        #[cfg(mobile)]
        let serving = async {};
        tokio::join!(serving, self.stop_observing(), self.status.stop());
    }

    async fn stop_observing(&self) {
        let router = self
            .observing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(router) = router
            && let Err(e) = router.shutdown().await
        {
            log::warn!("stopping mesh protocols: {e}");
        }
    }

    /// What is known of this Mac's copies of the machines' project folders.
    pub(crate) fn copies(&self) -> &Copies {
        &self.copies
    }

    /// The newest release of each tool a node installs.
    pub(crate) fn releases(&self) -> &ToolReleases {
        &self.releases
    }

    /// Holds `read` on `machine` while the receiver lives; see `Snapshots::follow`.
    pub(crate) fn snapshots_follow(
        &self,
        machine: EndpointId,
        read: beep_proto::node::events::Read,
    ) -> Option<std::result::Result<watch::Receiver<events::Current>, beep_mesh::MeshError>> {
        self.snapshots.follow(machine, read)
    }

    pub(crate) fn projections(&self) -> &node_actor::Projections {
        &self.projections
    }

    pub(crate) fn node_ops(&self) -> &Arc<node_actor::NodeOps> {
        &self.node_ops
    }

    pub fn id(&self) -> EndpointId {
        self.me
    }

    /// The endpoint this app reaches the mesh on.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The mesh, once it is open.
    pub fn mesh(&self) -> Option<Mesh> {
        watched::read_sent(&self.mesh, |opened| match opened {
            Opened::Open(mesh) => Some(mesh.clone()),
            _ => None,
        })
    }

    /// The mesh, waiting for it to open. Fails when it could not.
    pub async fn ready(&self) -> Result<Mesh> {
        let mut opened = self.mesh.subscribe();
        watched::wait_for(&mut opened, |opened| !matches!(opened, Opened::Opening))
            .await
            .map_err(|_| DesktopError::NoMesh("the app is closing".into()))?;
        let opened = watched::read(&opened, Clone::clone);
        match opened {
            Opened::Open(mesh) if self.session.is_none() && !mesh.is_member(self.me) => Err(
                DesktopError::NoMesh("this device was removed from the mesh".into()),
            ),
            Opened::Open(mesh) => Ok(mesh),
            #[cfg(mobile)]
            Opened::Unpaired => Err(DesktopError::NoMesh("this phone is not paired".into())),
            Opened::Failed(why) => Err(DesktopError::NoMesh(why)),
            Opened::Opening => Err(DesktopError::NoMesh("still opening".into())),
        }
    }

    /// The machines of the mesh, each with whether it is online. Empty until
    /// the mesh is open.
    pub fn machines(&self) -> Vec<DeviceStatus> {
        self.mesh().map(|mesh| mesh.machines()).unwrap_or_default()
    }

    /// What the machines of the mesh last reported.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// What the nodes say on the topics the windows and standing interests follow.
    pub(crate) fn heard(&self) -> tokio::sync::broadcast::Receiver<events::Heard> {
        self.snapshots.heard()
    }

    /// What each node last said on the topics `heard` carries, for a follower that lagged.
    pub(crate) fn heard_latest(&self) -> Vec<events::Heard> {
        self.snapshots.latest()
    }

    /// Wakes on each change to the mesh or to its list of machines.
    pub(crate) fn changes(&self) -> Changes {
        let mut opened = self.mesh.subscribe();
        let members = members(&mut opened);
        let mut status = self.status.changes();
        let machines = watched::read_and_update(&mut status, |status| status.machines);
        Changes {
            opened,
            members,
            status,
            machines,
        }
    }

    /// Mints a ticket that brings a device of `kind` into the mesh. Only a
    /// desktop can, and this Mac is one.
    pub async fn invite(&self, kind: DeviceKind) -> Result<JoinTicket> {
        Ok(self.ready().await?.invite(kind, DEFAULT_TICKET_TTL).await?)
    }

    /// Mints a ticket for a temporary node, such as a container: it is removed after a day
    /// offline.
    pub async fn invite_temporary(&self) -> Result<JoinTicket> {
        Ok(self
            .ready()
            .await?
            .invite_temporary(DEFAULT_TICKET_TTL)
            .await?)
    }

    /// Removes `machine` from the mesh for good. Every other member drops it
    /// and its link.
    pub async fn remove(&self, machine: EndpointId) -> Result<()> {
        if machine == self.me {
            return Err(DesktopError::BadRequest(
                "this computer cannot remove itself".into(),
            ));
        }

        Ok(self.ready().await?.remove(machine).await?)
    }

    /// Shows `machine` as `name` on every member; an empty name brings back the one it joined
    /// with. Agents address machines by name, so two cannot share one.
    pub async fn rename(&self, machine: EndpointId, name: &str) -> Result<()> {
        let name = name.trim();

        if name.chars().count() > MAX_MACHINE_NAME {
            return Err(DesktopError::BadRequest("that name is too long".into()));
        }

        let taken = self.machines().into_iter().any(|status| {
            status.device.id != machine && status.device.name.trim().eq_ignore_ascii_case(name)
        });

        if taken {
            return Err(DesktopError::BadRequest(format!(
                "another machine is called {name}"
            )));
        }

        let name = (!name.is_empty()).then(|| name.to_owned());
        Ok(self.ready().await?.rename(machine, name).await?)
    }

    /// Opens a phone's mesh, if it has one, and serves it.
    #[cfg(mobile)]
    async fn open_observer(&self) {
        match Mesh::open(&self.dir, self.endpoint.clone()).await {
            Ok(Some(mesh)) => self.observe(mesh).await,
            Ok(None) => {
                self.mesh.send_replace(Opened::Unpaired);
            }
            Err(e) => {
                log::warn!("not in a mesh: {e:#}");
                self.mesh.send_replace(Opened::Failed(e.to_string()));
            }
        }
    }

    /// Takes `mesh` as the phone's: marks the phone an observer, answers the mesh and join
    /// protocols, and follows the machines.
    #[cfg(mobile)]
    async fn observe(&self, mesh: Mesh) {
        match beep_fleet::observe(&mesh, &self.endpoint).await {
            Ok(router) => {
                *self
                    .observing
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(router);
                self.status.watch(mesh.clone()).await;
                self.mesh.send_replace(Opened::Open(mesh));
            }
            Err(e) => {
                log::warn!("not observing the mesh: {e:#}");
                self.mesh.send_replace(Opened::Failed(e.to_string()));
            }
        }
    }

    #[cfg(mobile)]
    /// Whether this phone belongs to a mesh, once it has looked. A phone the mesh removed is
    /// not paired, and `pair` may replace its mesh.
    pub async fn paired(&self) -> bool {
        let mut opened = self.mesh.subscribe();
        if watched::wait_for(&mut opened, |opened| !matches!(opened, Opened::Opening))
            .await
            .is_err()
        {
            return false;
        }

        let opened = watched::read(&opened, Clone::clone);

        matches!(&opened, Opened::Open(mesh) if mesh.is_member(self.me))
    }

    /// Joins the mesh a desktop's phone ticket names, as an observer. A phone the mesh removed
    /// leaves what it kept; a paired one is refused.
    #[cfg(mobile)]
    pub async fn pair(&self, text: &str, name: &str) -> Result<()> {
        let ticket =
            beep_fleet::phone_ticket(text).map_err(|e| DesktopError::BadRequest(e.to_string()))?;
        let _one = self.joining.lock().await;

        if self.paired().await {
            return Err(DesktopError::BadRequest(
                "this phone is already paired".into(),
            ));
        }
        // A removed phone's mesh still runs: stop it so its files can be replaced.
        let old = self.mesh();

        if let Some(old) = old {
            self.stop().await;

            if let Err(in_use) = close_within(old, CLOSE_WAIT).await {
                return Err(DesktopError::BadRequest(in_use.to_string()));
            }
        }

        let mesh =
            Mesh::join_replacing_lone(&self.dir, self.endpoint.clone(), ticket, name).await?;
        self.observe(mesh).await;
        self.ready().await.map(|_| ())
    }

    /// A client for `machine`. This Mac's own node is served in this process
    /// at once, with no network and no waiting. For another member the mesh
    /// gets `OPENING` to be open, then the machine gets `DEADLINE` to be
    /// connected, then it is offline.
    #[tracing::instrument(skip_all, fields(peer = %machine.fmt_short()))]
    pub async fn client(&self, machine: EndpointId) -> Result<PeerClient> {
        if machine == self.me {
            return match &self.session {
                Some(session) => Ok(PeerClient::local(session.clone())
                    .with_snapshot_reader(self.snapshots.reader(machine))),
                None => Err(DesktopError::BadRequest(
                    "This phone does not host sessions or files.".into(),
                )),
            };
        }

        let mesh = timeout(OPENING, self.ready())
            .await
            .map_err(|_| DesktopError::NoMesh("still opening".into()))??;
        if mesh.is_observer(machine) {
            return Err(DesktopError::BadRequest(
                "This device does not host sessions or files.".into(),
            ));
        }

        with_deadline(machine, &mesh)
            .await
            .map(|client| client.with_snapshot_reader(self.snapshots.reader(machine)))
            .map_err(|_| DesktopError::Offline(self.name_of(machine)))
    }

    /// Notes the phone app going to the background.
    #[cfg(mobile)]
    pub(crate) fn went_away(&self) {
        self.away
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(SystemTime::now);
    }

    /// The phone app is back. iroh binds and probes its paths again: iOS may
    /// have dropped its sockets while the app was suspended, and Android
    /// does not report network changes to native code. After `AWAY` the
    /// links may be dead while they look open, so each is dialed again.
    #[cfg(mobile)]
    pub(crate) async fn came_back(&self) {
        let away = self
            .away
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .map(|since| since.elapsed().unwrap_or_default());

        match away {
            Some(away) if away >= AWAY => self.reconnect().await,
            Some(_) => self.endpoint.network_change().await,
            None => {}
        }
    }

    /// The network changed, or the links may be dead: iroh probes the
    /// network again and every machine is dialed at once.
    pub(crate) async fn reconnect(&self) {
        match self.mesh() {
            Some(mesh) => mesh.reconnect().await,
            None => self.endpoint.network_change().await,
        }
    }

    /// `machine` as a member of the mesh, or this Mac. `None` for a machine
    /// the mesh does not hold.
    pub fn member(&self, machine: &str) -> Option<EndpointId> {
        let id: EndpointId = machine.parse().ok()?;
        (id == self.me || self.mesh()?.is_member(id)).then_some(id)
    }

    /// This app's key, which signs what it sends, such as a problem report.
    pub(crate) fn secret_key(&self) -> SecretKey {
        self.endpoint.secret_key().clone()
    }

    /// `member`, or `NotInMesh`.
    pub fn machine(&self, machine: &str) -> Result<EndpointId> {
        self.member(machine).ok_or(DesktopError::NotInMesh)
    }

    /// The members that are online now, this Mac among them.
    pub fn online(&self) -> Vec<EndpointId> {
        self.mesh()
            .map(|mesh| mesh.code_hosts())
            .unwrap_or_default()
            .into_iter()
            .filter(|status| {
                if status.device.id == self.me {
                    self.session.is_some()
                } else {
                    status.online
                }
            })
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

#[cfg(all(test, desktop))]
#[path = "../tests/fleet.rs"]
mod tests;
