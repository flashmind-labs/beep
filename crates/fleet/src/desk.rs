//! What every call on the desktop needs: the mesh, the machines in it with a shell on each that is
//! online, and the git hosts.

use crate::{
    error::{FleetError, Result},
    peer::{PeerFound, PeerShell, Peers, probe},
};
use async_trait::async_trait;
use beep_mesh::{DeviceStatus, Mesh};
use beep_projects::{
    imports::{hosts::Hosts, machine::Machine},
    model::ForgejoUrl,
    repo_watch::http::HostCalls,
};
use beep_proto::node::{EnvReport, agent::SESSIONS_PROTOCOL, shell};
use futures::future::join_all;
use iroh::EndpointId;
use reqwest::{Client, redirect::Policy};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Semaphore, time::timeout};

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
        Self::with(peers, DesktopHosts::new()?).await
    }

    /// A desk over `peers` that shares `hosts` with the others: one client for the session.
    pub(crate) async fn with(peers: Arc<dyn Peers>, hosts: DesktopHosts) -> Result<Self> {
        let mesh = peers.mesh().await?;
        Ok(Self {
            local: peers.me(),
            peers,
            mesh,
            hosts,
            settle: crate::usage::SETTLE,
            github_api: None,
        })
    }

    /// Asks GitHub's API at `base` instead of github.com.
    pub fn github_api(mut self, base: &str) -> Self {
        self.github_api = Some(base.to_owned());
        self
    }

    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// Every machine of the mesh, with a shell on each that is online.
    pub(crate) async fn machines(&self) -> Vec<Machine> {
        self.seen()
            .await
            .into_iter()
            .map(|seen| seen.machine)
            .collect()
    }

    /// Every machine of the mesh with what it said about itself.
    pub(crate) async fn seen(&self) -> Vec<Seen> {
        join_all(self.mesh.devices().into_iter().map(|d| self.look(d))).await
    }

    /// Only the machines among `ids`: a read of one project has no use for the others, and one
    /// that is slow to answer is not waited on.
    pub(crate) async fn machines_among(&self, ids: &[EndpointId]) -> Vec<Machine> {
        let wanted = self
            .mesh
            .devices()
            .into_iter()
            .filter(|status| ids.contains(&status.device.id));
        join_all(wanted.map(|status| self.look(status)))
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
            .devices()
            .into_iter()
            .find(|status| status.device.id == machine)?;
        Some(self.look(status).await)
    }

    async fn look(&self, status: DeviceStatus) -> Seen {
        let id: EndpointId = status.device.id;
        let mut machine = Machine {
            id: id.to_string(),
            name: status.device.name,
            online: status.online,
            os: None,
            home: None,
            workspace: None,
            shell: None,
            sessions: None,
        };
        if !status.online {
            return Seen { machine, env: None };
        }
        let ask = async {
            let client = self.peers.client(id).await?;
            probe(&client).await.map(|said| (client, said))
        };
        let said = timeout(LOOK, ask)
            .await
            .unwrap_or_else(|_| Err(FleetError::Refused("no answer in time".to_owned())));
        let mut env = None;
        match said {
            Ok((client, said)) => {
                if said.offers(shell::PROTOCOL) {
                    machine.shell = Some(Arc::new(PeerShell::new(client.clone())));
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

/// Calls to git hosts from this Mac, shared by every call of the session: one client, one limit
/// on calls in flight, and the replies and rate limits kept for the runs.
#[derive(Clone)]
pub(crate) struct DesktopHosts {
    client: Client,
    calls: Arc<Semaphore>,
    kept: Arc<HostCalls>,
}

impl DesktopHosts {
    pub(crate) fn new() -> Result<Self> {
        let client = Client::builder()
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
        })
    }

    pub(crate) fn reqwest(&self) -> Result<Client> {
        Ok(self.client.clone())
    }

    pub(crate) fn semaphore(&self) -> &Arc<Semaphore> {
        &self.calls
    }

    pub(crate) fn host_calls(&self) -> &HostCalls {
        &self.kept
    }
}

#[async_trait]
impl Hosts for DesktopHosts {
    fn client(&self) -> anyhow::Result<Client> {
        Ok(self.client.clone())
    }

    fn calls(&self) -> &Arc<Semaphore> {
        &self.calls
    }

    /// Every address is called: the desktop is on the user's own network, which is the point of
    /// calling a Forgejo server there. The server refuses private addresses because it must not
    /// probe the network it runs in; a desktop has no such network to protect.
    async fn is_public(&self, _: &ForgejoUrl) -> bool {
        true
    }
}
