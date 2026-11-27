//! What every call on the desktop needs: the mesh, the machines in it with a shell on each that is
//! online, and the git hosts.

use crate::{
    AsyncMutex,
    changed::Changed,
    error::{FleetError, Result},
    peer::{PeerFound, PeerShell, Peers, Probe, probe},
};
use beep_mesh::{
    DeviceStatus, Mesh,
    machine::{search, sessions::MoveState},
};
use beep_projects::{
    imports::{hosts::Hosts, machine::Machine},
    is_windows,
    issues::HostGates,
    repo_watch::http::HostCalls,
};
use beep_proto::{
    SessionId,
    node::{EnvReport, agent::SESSIONS_PROTOCOL, shell},
};
use futures::future::join_all;
use iroh::EndpointId;
use reqwest::{Client, redirect::Policy};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::{
    sync::{Semaphore, mpsc::UnboundedSender},
    time::timeout,
};

/// Calls to git hosts in flight at once.
const MAX_CALLS: usize = 8;
const HOST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a machine the mesh says is online may take to say what it is: its link and its report,
/// so one that does not answer holds nothing up for longer.
const LOOK: Duration = Duration::from_secs(6);

/// The desktop's view of the mesh for one call.
pub struct Desk {
    pub(crate) peers: Arc<dyn Peers>,
    pub(crate) mesh: Mesh,
    pub(crate) hosts: DesktopHosts,
    /// This Mac's own node, which answers at once.
    pub(crate) local: EndpointId,
    /// How long a usage read waits on a quiet node.
    pub(crate) settle: Duration,
    /// Where GitHub's API is, when it is not github.com.
    pub(crate) github_api: Option<String>,
}

impl Desk {
    /// A desk over `peers`, once its mesh is open.
    pub async fn open(peers: Arc<dyn Peers>) -> Result<Self> {
        let (changes, _) = tokio::sync::mpsc::unbounded_channel();
        Self::with(peers, DesktopHosts::new(changes)?).await
    }

    /// A desk over `peers` that shares `hosts` with the others: one client for the session.
    pub(crate) async fn with(peers: Arc<dyn Peers>, hosts: DesktopHosts) -> Result<Self> {
        let mesh = peers.mesh().await?;
        Ok(Self {
            local: peers.me(),
            peers,
            mesh,
            hosts,
            settle: crate::usage_cache::SETTLE,
            github_api: None,
        })
    }

    /// The mesh's id: what other meshes know it by.
    pub fn mesh_id(&self) -> EndpointId {
        self.mesh.id()
    }

    /// Asks GitHub's API at `base` instead of github.com.
    pub fn github_api(mut self, base: &str) -> Self {
        self.github_api = Some(base.to_owned());
        self
    }

    /// Every code host of the mesh, with a shell on each that is online.
    pub(crate) async fn machines(&self) -> Vec<Machine> {
        self.seen()
            .await
            .into_iter()
            .map(|seen| seen.machine)
            .collect()
    }

    /// Every code host of the mesh with what it said about itself.
    pub(crate) async fn seen(&self) -> Vec<Seen> {
        self.look_all(self.mesh.code_hosts()).await
    }

    /// Only the machines among `ids`: a read of one project has no use for the others, and one
    /// that is slow to answer is not waited on.
    pub(crate) async fn machines_among(&self, ids: &[EndpointId]) -> Vec<Machine> {
        let wanted = self
            .mesh
            .code_hosts()
            .into_iter()
            .filter(|status| ids.contains(&status.device.id))
            .collect();
        self.look_all(wanted)
            .await
            .into_iter()
            .map(|seen| seen.machine)
            .collect()
    }

    /// Only this Mac's own machine, without asking the others anything.
    pub(crate) async fn local_machine(&self) -> Option<Machine> {
        Some(self.seen_one(self.local).await?.machine)
    }

    /// One machine of the mesh with what it said about itself, without asking the others.
    pub(crate) async fn seen_one(&self, machine: EndpointId) -> Option<Seen> {
        let status = self
            .mesh
            .code_hosts()
            .into_iter()
            .find(|status| status.device.id == machine)?;
        Some(self.look(status).await)
    }

    /// Fills in the "Projects in" folder of each machine with a shell, only those among `ids` when
    /// given; one that does not say keeps its home as the default.
    pub(crate) async fn learn_projects_in(&self, machines: &mut [Machine], ids: Option<&[&str]>) {
        let asks: Vec<_> = machines
            .iter()
            .enumerate()
            .filter(|(_, m)| m.shell.is_some() && ids.is_none_or(|ids| ids.contains(&&*m.id)))
            .filter_map(|(at, m)| Some((at, m.id.parse::<EndpointId>().ok()?)))
            .map(|(at, id)| async move {
                let ask = async {
                    let client = self.peers.client(id).await?;
                    Ok::<_, FleetError>(search(&client).await?)
                };
                (at, within_look(ask).await)
            })
            .collect();

        for (at, said) in join_all(asks).await {
            match said {
                Ok(folder) => machines[at].projects_in = Some(folder),
                Err(e) => log::info!("{} did not say its projects folder: {e}", machines[at].name),
            }
        }
    }

    async fn look_all(&self, statuses: Vec<DeviceStatus>) -> Vec<Seen> {
        join_all(statuses.into_iter().map(|status| self.look(status))).await
    }

    pub(crate) async fn seen_among(&self, ids: &[EndpointId]) -> Vec<Seen> {
        let wanted = self
            .mesh
            .code_hosts()
            .into_iter()
            .filter(|status| ids.contains(&status.device.id))
            .collect();
        self.look_all(wanted).await
    }

    async fn look(&self, status: DeviceStatus) -> Seen {
        let id: EndpointId = status.device.id;
        let mut machine = Machine {
            id: id.to_string(),
            name: status.device.name,
            online: status.online,
            os: None,
            home: None,
            projects_in: None,
            shell: None,
            sessions: None,
        };

        if !status.online {
            return Seen { machine, env: None };
        }

        let cached_offers = self.peers.last_offers(id);
        let cached_env = self.peers.last_env(id);
        let said = match (cached_offers, cached_env) {
            (Some(offers), Some(env)) => {
                within_look(async {
                    let client = self.peers.client(id).await?;
                    Ok((
                        client,
                        Probe {
                            offers,
                            env: Some(env),
                        },
                    ))
                })
                .await
            }
            (offers, env) => {
                let ask = async {
                    let client = self.peers.client(id).await?;
                    let mut probed = probe(&client).await?;

                    if probed.offers.is_empty()
                        && let Some(offers) = offers
                    {
                        probed.offers = offers;
                    }
                    if probed.env.is_none() {
                        probed.env = env;
                    }

                    Ok((client, probed))
                };
                within_look(ask).await
            }
        };
        let mut env = None;

        match said {
            Ok((client, said)) => {
                if said.offers(shell::PROTOCOL) {
                    let windows = said.env.as_ref().is_some_and(|env| is_windows(&env.os));
                    machine.shell = Some(Arc::new(PeerShell::new(client.clone(), windows)));
                }
                if said.offers(SESSIONS_PROTOCOL) {
                    machine.sessions = Some(Arc::new(PeerFound::new(client)));
                }
                machine.os = said.env.as_ref().map(|env| env.os.clone());
                machine.home = said.env.as_ref().map(|env| env.home.clone());
                env = said.env;
            }
            Err(e) => log::info!("{} did not say what it is: {e}", machine.name),
        }
        Seen { machine, env }
    }
}

/// A machine as the project code sees it, and what it reported about itself.
pub(crate) struct Seen {
    pub(crate) machine: Machine,
    pub(crate) env: Option<EnvReport>,
}

/// A checkout's remote URLs and when they were read.
type KeptRemotes = (Instant, Vec<String>);

/// Calls to git hosts from this Mac, shared by every call of the session: one client, one limit
/// on calls in flight, and the replies and rate limits kept for the runs.
#[derive(Clone)]
pub(crate) struct DesktopHosts {
    client: Client,
    calls: Arc<Semaphore>,
    kept: Arc<HostCalls>,
    /// Each project's checkout remotes and when they were read, for the hosts its runs are on.
    remotes: Arc<Mutex<HashMap<String, KeptRemotes>>>,
    /// The issue hosts each machine could not reach lately.
    gates: Arc<HostGates>,
    /// Held by the run reading Home's pull requests and issues.
    board_run: Arc<AsyncMutex<()>>,
    /// What nodes that push no sync status last said of it.
    direct_syncs: Arc<crate::direct_syncs::DirectSyncs>,
    /// Per-machine spend cache, so a degraded node keeps its last spend instead of dropping to 0.
    usage: Arc<crate::usage_cache::UsageCache>,
    /// Last move state reported by each target machine, used when a link glitches momentarily.
    moves: Arc<Mutex<HashMap<SessionId, MoveState>>>,
}

impl DesktopHosts {
    /// `changes` hears `Changed::Sync` when a node that pushes no sync status said something new
    /// of it.
    pub(crate) fn new(changes: UnboundedSender<Changed>) -> Result<Self> {
        let client = beep_proto::https::client()
            .connect_timeout(Duration::from_secs(3))
            .timeout(HOST_TIMEOUT)
            .user_agent("beep-desktop")
            // A redirect could send the token anywhere.
            .redirect(Policy::none())
            .build()
            .map_err(anyhow::Error::from)?;
        Ok(Self {
            client,
            calls: Arc::new(Semaphore::new(MAX_CALLS)),
            kept: Arc::default(),
            remotes: Arc::default(),
            gates: Arc::default(),
            board_run: Arc::default(),
            direct_syncs: Arc::new(crate::direct_syncs::DirectSyncs::new(changes)),
            usage: Arc::default(),
            moves: Arc::default(),
        })
    }

    pub(crate) fn direct_syncs(&self) -> &Arc<crate::direct_syncs::DirectSyncs> {
        &self.direct_syncs
    }

    pub(crate) fn usage(&self) -> &Arc<crate::usage_cache::UsageCache> {
        &self.usage
    }

    pub(crate) fn move_states(&self) -> &Arc<Mutex<HashMap<SessionId, MoveState>>> {
        &self.moves
    }

    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    pub(crate) fn semaphore(&self) -> &Arc<Semaphore> {
        &self.calls
    }

    pub(crate) fn host_calls(&self) -> &HostCalls {
        &self.kept
    }

    pub(crate) fn gates(&self) -> &Arc<HostGates> {
        &self.gates
    }

    pub(crate) fn board_run(&self) -> &AsyncMutex<()> {
        &self.board_run
    }

    /// `project`'s remotes as read less than `within` ago.
    pub(crate) fn remotes(&self, project: &str, within: Duration) -> Option<Vec<String>> {
        let kept = self.remotes.lock().unwrap_or_else(PoisonError::into_inner);
        kept.get(project)
            .filter(|(at, _)| at.elapsed() < within)
            .map(|(_, urls)| urls.clone())
    }

    pub(crate) fn keep_remotes(&self, project: &str, urls: Vec<String>) {
        let mut kept = self.remotes.lock().unwrap_or_else(PoisonError::into_inner);
        kept.insert(project.to_owned(), (Instant::now(), urls));
    }
}

impl Hosts for DesktopHosts {
    fn client(&self) -> anyhow::Result<Client> {
        Ok(DesktopHosts::client(self).clone())
    }

    fn calls(&self) -> &Arc<Semaphore> {
        &self.calls
    }
}

/// `ask`, or a refusal when it takes longer than [`LOOK`].
async fn within_look<T>(ask: impl Future<Output = Result<T>>) -> Result<T> {
    timeout(LOOK, ask)
        .await
        .unwrap_or_else(|_| Err(FleetError::Refused("no answer in time".to_owned())))
}
