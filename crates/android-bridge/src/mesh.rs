//! A phone is a mesh controller. Sessions and checkouts stay on their machines.

use crate::{AsyncMutex, BeepClient, BeepError, bad_request};
use async_trait::async_trait;
use beep_fleet::{Desk, FleetError, Peers};
use beep_mesh::{Mesh, MeshError, PeerClient};
use iroh::{Endpoint, EndpointId, SecretKey, endpoint::presets, protocol::Router};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::OnceCell;

mod agents;
mod alerts;
mod home;
mod projects;
mod status;
mod terminals;

impl From<MeshError> for BeepError {
    fn from(error: MeshError) -> Self {
        Self::Connection {
            reason: error.to_string(),
        }
    }
}

impl From<FleetError> for BeepError {
    fn from(error: FleetError) -> Self {
        match error {
            FleetError::NotFound => Self::NotFound,
            FleetError::Refused(reason) => bad_request(reason),
            FleetError::Conflict(reason) => Self::Conflict { reason },
            other => Self::Connection {
                reason: other.to_string(),
            },
        }
    }
}

pub(super) struct Workspace {
    directory: PathBuf,
    key: SecretKey,
    endpoint: OnceCell<Endpoint>,
    state: AsyncMutex<State>,
}

#[derive(Default)]
struct State {
    loaded: bool,
    linked: Option<Arc<Connected>>,
}

pub(super) struct Connected {
    pub mesh: Mesh,
    // Own the router for exactly as long as the mesh is in use.
    _router: Router,
}

impl Workspace {
    pub fn new(directory: PathBuf, key: SecretKey) -> Self {
        Self {
            directory: directory.join(key.public().to_string()),
            key,
            endpoint: OnceCell::new(),
            state: AsyncMutex::default(),
        }
    }

    async fn endpoint(&self) -> Result<&Endpoint, BeepError> {
        self.endpoint
            .get_or_try_init(|| async {
                Endpoint::builder(presets::N0)
                    .secret_key(self.key.clone())
                    .transport_config(beep_mesh::transport_config())
                    .bind()
                    .await
                    .map_err(|error| BeepError::Connection {
                        reason: error.to_string(),
                    })
            })
            .await
    }

    pub async fn network_changed(&self) {
        if let Some(endpoint) = self.endpoint.get() {
            endpoint.network_change().await;
        }
    }

    async fn get(&self) -> Result<Option<Arc<Connected>>, BeepError> {
        let mut state = self.state.lock().await;
        if !state.loaded {
            if let Some(mesh) = Mesh::open(&self.directory, self.endpoint().await?.clone()).await? {
                state.linked = Some(Connected::new(mesh, self.endpoint().await?).await?);
            }
            state.loaded = true;
        }
        if let Some(connected) = &state.linked
            && !connected.mesh.is_member(self.key.public())
        {
            return Err(BeepError::Unauthorized);
        }
        Ok(state.linked.clone())
    }

    pub async fn join(&self, text: &str, name: &str) -> Result<(), BeepError> {
        let ticket = beep_fleet::phone_ticket(text)?;
        self.get().await?;
        let mut state = self.state.lock().await;
        if state.linked.is_some() {
            return Err(bad_request("This phone is already linked to a workspace."));
        }
        let endpoint = self.endpoint().await?;
        let mesh = Mesh::join(&self.directory, endpoint.clone(), &ticket, name).await?;
        state.linked = Some(Connected::new(mesh, endpoint).await?);
        state.loaded = true;
        Ok(())
    }
}

impl Connected {
    async fn new(mesh: Mesh, endpoint: &Endpoint) -> Result<Arc<Self>, BeepError> {
        let router = beep_fleet::observe(&mesh, endpoint).await?;
        Ok(Arc::new(Self {
            mesh,
            _router: router,
        }))
    }

    pub async fn desk(self: &Arc<Self>) -> Result<Desk, BeepError> {
        Ok(Desk::open(self.clone()).await?)
    }

    pub async fn client(&self, id: &str) -> Result<PeerClient, BeepError> {
        let id = id
            .parse()
            .map_err(|_| bad_request("Invalid machine identity"))?;
        if id == self.mesh.me() || !self.mesh.is_member(id) {
            return Err(BeepError::NotFound);
        }
        beep_mesh::with_deadline(id, &self.mesh)
            .await
            .map_err(|error| BeepError::Connection {
                reason: error.to_string(),
            })
    }
}

#[async_trait]
impl Peers for Connected {
    async fn mesh(&self) -> Result<Mesh, FleetError> {
        Ok(self.mesh.clone())
    }
    fn me(&self) -> EndpointId {
        self.mesh.me()
    }
    async fn client(&self, machine: EndpointId) -> Result<PeerClient, FleetError> {
        self.client(&machine.to_string())
            .await
            .map_err(|error| match error {
                BeepError::NotFound => FleetError::NotFound,
                other => FleetError::Other(other.into()),
            })
    }
}

impl BeepClient {
    pub(super) async fn mesh(&self) -> Result<Option<Arc<Connected>>, BeepError> {
        match self.workspace.get() {
            Some(workspace) => workspace.get().await,
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests;
