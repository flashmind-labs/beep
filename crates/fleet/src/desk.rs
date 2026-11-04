//! What every call on the desktop needs: the mesh, the machines in it with a shell on each that is
//! online, and the git hosts.

use crate::{
    error::Result,
    peer::{PeerFound, PeerShell, Peers, probe},
};
use async_trait::async_trait;
use beep_mesh::{DeviceStatus, Mesh};
use beep_projects::{
    imports::{hosts::Hosts, machine::Machine},
    model::ForgejoUrl,
};
use beep_proto::node::{EnvReport, agent::SESSIONS_PROTOCOL, shell};
use futures::future::join_all;
use iroh::EndpointId;
use reqwest::{Client, redirect::Policy};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

/// Calls to git hosts in flight at once.
const MAX_CALLS: usize = 8;
const HOST_TIMEOUT: Duration = Duration::from_secs(15);

/// The desktop's view of the mesh for one call.
pub struct Desk {
    pub(crate) peers: Arc<dyn Peers>,
    pub(crate) mesh: Mesh,
    pub(crate) hosts: DesktopHosts,
    /// How long a usage read waits on a quiet node.
    pub(crate) settle: Duration,
    /// Where GitHub's API is, when it is not github.com.
    pub(crate) github_api: Option<String>,
}

impl Desk {
    /// A desk over `peers`, once its mesh is open.
    pub async fn open(peers: Arc<dyn Peers>) -> Result<Self> {
        let mesh = peers.mesh().await?;
        Ok(Self {
            peers,
            mesh,
            hosts: DesktopHosts::new()?,
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
        let said = match self.peers.client(id).await {
            Ok(client) => probe(&client).await.map(|said| (client, said)),
            Err(e) => Err(e),
        };
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

/// Calls to git hosts from this Mac: its own network is the user's, so every host may be called.
pub(crate) struct DesktopHosts {
    client: Client,
    calls: Arc<Semaphore>,
}

impl DesktopHosts {
    fn new() -> Result<Self> {
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
        })
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

    async fn is_public(&self, _: &ForgejoUrl) -> bool {
        true
    }
}
