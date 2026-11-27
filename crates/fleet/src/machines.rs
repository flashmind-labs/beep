//! The machines of the mesh as the session reaches them: the mesh once it is open, a client for
//! each member, and what each machine last said about itself.

use crate::error::{FleetError, Result};
use beep_mesh::{Mesh, PeerClient, SessionFn, Snapshots, with_deadline};
use beep_proto::{node::EnvReport, watched};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{sync::watch, time::timeout};

/// How long a call to another machine waits for the mesh to open.
const OPENING: Duration = Duration::from_secs(10);

/// Where the mesh stands.
#[derive(Clone)]
pub enum MeshState {
    Opening,
    /// A phone with no mesh yet: it waits for a desktop's phone ticket.
    Unpaired,
    Open(Mesh),
    /// It could not be opened or made; the text says why.
    Failed(String),
}

/// The mesh and the machines in it, owned by the app that hosts the session: it sets the mesh
/// and what each machine reported, and the session reads them.
pub struct Machines {
    me: EndpointId,
    /// This machine's own node, served in this process. None on a phone.
    local: Option<SessionFn>,
    opened: watch::Sender<MeshState>,
    snapshots: Arc<Snapshots>,
    env: Mutex<HashMap<EndpointId, EnvReport>>,
    offers: Mutex<HashMap<EndpointId, Vec<String>>>,
    /// Nodes in this process that tests stand in for other machines.
    #[cfg(any(test, feature = "test-util"))]
    nodes: Mutex<HashMap<EndpointId, StandIn>>,
}

impl Machines {
    /// The machines of the mesh of `me`, whose own node is `local`. The mesh is opening.
    pub fn new(me: EndpointId, local: Option<SessionFn>) -> Self {
        Self {
            me,
            local,
            opened: watch::channel(MeshState::Opening).0,
            snapshots: Arc::default(),
            env: Mutex::default(),
            offers: Mutex::default(),
            #[cfg(any(test, feature = "test-util"))]
            nodes: Mutex::default(),
        }
    }

    /// This machine's own id.
    pub fn me(&self) -> EndpointId {
        self.me
    }

    /// Whether this machine runs a node of its own.
    pub fn hosts(&self) -> bool {
        self.local.is_some()
    }

    /// What the nodes pushed, read by the clients this hands out.
    pub fn snapshots(&self) -> &Arc<Snapshots> {
        &self.snapshots
    }

    /// Tells where the mesh stands.
    pub fn set_opened(&self, opened: MeshState) {
        self.opened.send_replace(opened);
    }

    /// Follows where the mesh stands.
    pub fn opened(&self) -> watch::Receiver<MeshState> {
        self.opened.subscribe()
    }

    /// The mesh, once it is open.
    pub fn mesh_now(&self) -> Option<Mesh> {
        watched::read_sent(&self.opened, |opened| match opened {
            MeshState::Open(mesh) => Some(mesh.clone()),
            _ => None,
        })
    }

    /// The mesh, waiting for it to open. Fails when it could not.
    pub async fn mesh(&self) -> Result<Mesh> {
        let mut opened = self.opened.subscribe();
        watched::wait_for(&mut opened, |opened| !matches!(opened, MeshState::Opening))
            .await
            .map_err(|_| FleetError::NoMesh("the app is closing".into()))?;
        let opened = watched::read(&opened, Clone::clone);

        match opened {
            MeshState::Open(mesh) if self.local.is_none() && !mesh.is_member(self.me) => Err(
                FleetError::NoMesh("this device was removed from the mesh".into()),
            ),
            MeshState::Open(mesh) => Ok(mesh),
            MeshState::Unpaired => Err(FleetError::NoMesh("this phone is not paired".into())),
            MeshState::Failed(why) => Err(FleetError::NoMesh(why)),
            MeshState::Opening => Err(FleetError::NoMesh("still opening".into())),
        }
    }

    /// A client for `machine`. This machine's own node is served in this process at once, with
    /// no network and no waiting. For another member the mesh gets `OPENING` to be open, then
    /// the machine gets `DEADLINE` to be connected, then it is offline.
    #[tracing::instrument(skip_all, fields(peer = %machine.fmt_short()))]
    pub async fn client(&self, machine: EndpointId) -> Result<PeerClient> {
        #[cfg(any(test, feature = "test-util"))]
        if let Some(stand_in) = self.node(machine) {
            let session = match stand_in {
                StandIn::Node(session) => session,
                StandIn::NodeWhen(session, mut open) => {
                    let _ = watched::wait_for(&mut open, |open| *open).await;
                    session
                }
                StandIn::Stalled => return std::future::pending().await,
            };

            return Ok(PeerClient::local(session).with_snapshots(self.snapshots.clone(), machine));
        }

        if machine == self.me {
            return match &self.local {
                Some(session) => Ok(PeerClient::local(session.clone())
                    .with_snapshots(self.snapshots.clone(), machine)),
                None => Err(FleetError::refused(
                    "This phone does not host sessions or files.",
                )),
            };
        }

        let mesh = timeout(OPENING, self.mesh())
            .await
            .map_err(|_| FleetError::NoMesh("still opening".into()))??;
        if mesh.is_observer(machine) {
            return Err(FleetError::refused(
                "This device does not host sessions or files.",
            ));
        }

        with_deadline(machine, &mesh)
            .await
            .map(|client| client.with_snapshots(self.snapshots.clone(), machine))
            .map_err(|_| FleetError::Offline(self.name_of(machine)))
    }

    /// The name of `machine` in the mesh, or its short id.
    pub fn name_of(&self, machine: EndpointId) -> String {
        self.mesh_now()
            .and_then(|mesh| {
                mesh.machines()
                    .into_iter()
                    .find(|status| status.device.id == machine)
            })
            .map_or_else(
                || machine.fmt_short().to_string(),
                |status| status.device.name,
            )
    }

    /// What `machine` last said about itself, kept from before: its probe is slow, and an
    /// offline machine says nothing now.
    pub fn last_env(&self, machine: EndpointId) -> Option<EnvReport> {
        self.env().get(&machine).cloned()
    }

    /// The protocols `machine` last reported offering on `status/1`.
    pub fn last_offers(&self, machine: EndpointId) -> Option<Vec<String>> {
        self.offers()
            .get(&machine)
            .filter(|offers| !offers.is_empty())
            .cloned()
    }

    /// Whether `machine` last reported offering `protocol`.
    pub fn offers_protocol(&self, machine: EndpointId, protocol: &str) -> bool {
        self.offers()
            .get(&machine)
            .is_some_and(|offers| offers.iter().any(|offer| offer == protocol))
    }

    /// Keeps what `machine` reported about itself; whether that is new.
    pub fn set_env(&self, machine: EndpointId, env: EnvReport) -> bool {
        self.env().insert(machine, env.clone()) != Some(env)
    }

    /// Keeps the protocols `machine` reported offering; whether that is new.
    pub fn set_offers(&self, machine: EndpointId, offers: Vec<String>) -> bool {
        self.offers().insert(machine, offers.clone()) != Some(offers)
    }

    /// Lets go of what is kept of the machines `keep` does not hold.
    pub fn keep_only(&self, keep: impl Fn(EndpointId) -> bool) {
        self.env().retain(|machine, _| keep(*machine));
        self.offers().retain(|machine, _| keep(*machine));
    }

    fn env(&self) -> MutexGuard<'_, HashMap<EndpointId, EnvReport>> {
        self.env.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn offers(&self) -> MutexGuard<'_, HashMap<EndpointId, Vec<String>>> {
        self.offers.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(any(test, feature = "test-util"))]
impl Machines {
    /// The machines of `mesh`, open, as a desktop that hosts no node of its own sees them.
    pub fn for_tests(mesh: Mesh) -> Arc<Self> {
        let machines = Self::new(mesh.me(), None);
        machines.set_opened(MeshState::Open(mesh));

        Arc::new(machines)
    }

    /// Has `machine` stood in for by `stand_in`, a test's node for it.
    pub fn stand_in(&self, machine: EndpointId, stand_in: StandIn) {
        self.nodes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(machine, stand_in);
    }

    fn node(&self, machine: EndpointId) -> Option<StandIn> {
        self.nodes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&machine)
            .cloned()
    }
}

/// What a test has serve a machine.
#[cfg(any(test, feature = "test-util"))]
#[derive(Clone)]
pub enum StandIn {
    /// A node in this process.
    Node(SessionFn),
    /// A node whose client may be had once the flag says true.
    NodeWhen(SessionFn, watch::Receiver<bool>),
    /// A machine that never answers a request for its client, to show that a call does not wait.
    Stalled,
}
