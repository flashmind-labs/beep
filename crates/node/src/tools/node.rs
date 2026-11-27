//! What the tools read of a node.

use super::ProjectTools;
use crate::{
    agents::Agents, files::transfer::Sharing, machine::service::logs::Logs, membership::Membership,
    status::Own, team::link::TeamLink,
};
use std::sync::Arc;

/// What the tools read of this node, which `link` builds from what it
/// serves: the sessions, the mesh and the node's own values.
#[derive(Clone)]
pub struct Node {
    pub(crate) agents: Arc<Agents>,
    pub(crate) mesh: Membership,
    pub(crate) transfers: Sharing,
    pub(crate) team: TeamLink,
    pub(crate) project_tools: Option<Arc<dyn ProjectTools>>,
    pub(crate) own: Own,
    /// The node's log, which `beep_logs` reads through the report targets.
    pub(crate) log: Logs,
}
