//! What `beep-node status` shows, as the running node reports it on its
//! control socket. Compact msgpack read by position with
//! `beep_proto::lenient`: a new field goes at the end with
//! `#[serde(default)]`, and none is ever inserted, reordered or removed.

use beep_mesh::DeviceKind;
use beep_proto::node::sync::ProjectStatus;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};

/// The running node as it sees itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStatus {
    pub version: String,
    /// When the node started, in seconds since the Unix epoch.
    pub started: u64,
    pub name: String,
    pub id: EndpointId,
    /// The mesh's id; `None` while the node is in none.
    pub mesh: Option<EndpointId>,
    /// Every device of the mesh, this one included.
    pub devices: Vec<Peer>,
    pub projects: Vec<Project>,
}

/// A device of the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub id: EndpointId,
    pub name: String,
    pub kind: DeviceKind,
    pub online: bool,
    /// When its link to this node last ended, in seconds since the Unix
    /// epoch; `None` while online or when the node has not reached it.
    pub last_seen: Option<u64>,
}

/// A project this node syncs, or would.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub status: ProjectStatus,
    /// Syncthing keeps the folder in step: Beep waits until it lets go.
    pub held: bool,
    /// The folder is not on this machine.
    pub missing: bool,
}
