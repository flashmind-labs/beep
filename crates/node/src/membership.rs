//! The mesh the node has joined, once it has, and what it holds for the node
//! to use: credentials and the servers they name.

use beep_mesh::{Key, Mesh};
use beep_proto::node::{MachineCredentials, mesh_state::{Credential, StateValue}};
use beep_proto::watched;
use std::sync::Arc;
use tokio::sync::watch;

/// The mesh the node belongs to, once it does.
#[derive(Clone)]
pub struct Membership {
    mesh: Arc<watch::Sender<Option<Mesh>>>,
}

impl Default for Membership {
    fn default() -> Self {
        Self {
            mesh: Arc::new(watch::Sender::new(None)),
        }
    }
}

impl Membership {
    pub(crate) fn set(&self, mesh: Mesh) {
        self.mesh.send_replace(Some(mesh));
    }

    pub(crate) fn clear(&self) {
        self.mesh.send_replace(None);
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<Mesh>> {
        self.mesh.subscribe()
    }

    pub(crate) fn get(&self) -> Option<Mesh> {
        watched::read_sent(&self.mesh, Clone::clone)
    }
}

/// The values under `prefix` that read as `T`; one that does not is left
/// out and its key logged, never its content.
pub(crate) fn read<T: StateValue>(mesh: &Mesh, prefix: &str) -> Vec<(Key, T)> {
    mesh.scan(prefix)
        .into_iter()
        .filter_map(|(key, bytes)| match T::from_bytes(&bytes) {
            Ok(value) => Some((key, value)),
            Err(_) => {
                log::warn!("ignored {key} in the mesh: it does not read");
                None
            }
        })
        .collect()
}

/// The base URLs of the Forgejo and Gitea servers the mesh holds logins for.
pub(crate) fn forgejo_servers(mesh: &Mesh) -> Vec<String> {
    credentials(mesh)
        .forgejo
        .into_iter()
        .map(|login| login.url)
        .collect()
}

/// The credentials the node uses.
pub(crate) fn credentials(mesh: &Mesh) -> MachineCredentials {
    let mut out = MachineCredentials::default();

    for (_, credential) in read::<Credential>(mesh, "credential/") {
        match credential {
            Credential::Env(secret) => out.env.push(secret),
            Credential::Forgejo(login) => out.forgejo.push(login),
            Credential::Agent(login) => out.agents.push(login),
        }
    }

    out
}
