//! The mesh the node has joined, once it has.

use beep_mesh::Mesh;
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
