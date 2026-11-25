//! Who calls a tool of this machine, and what it lets them do.

use super::{Agents, desktop_call::ProjectTools};
use crate::{
    files::transfer::Transfers,
    link::Offers,
    run::tasks::Table,
    team::link::TeamLink,
};
use beep_mesh::Mesh;
use std::sync::Arc;

/// Who calls, and what this device lets them do.
#[derive(Clone)]
pub(crate) struct Caller {
    /// The tasks the caller's jobs live in and can see.
    pub table: Table,
    /// Whether this device is a desktop, used for team roster and invite writes.
    pub desktop_here: bool,
    /// Why a node may not run commands or start agents here, if the host
    /// does not share this machine with other devices.
    pub commands_off: Option<String>,
    /// This node's synced projects and transfers, when it shares folders.
    pub transfers: Option<Transfers>,
    /// This node's coding agents, which another node's agents may start
    /// agents among.
    pub agents: Option<Arc<Agents>>,
    /// This machine's answer to the assistant's project tools, when it is a
    /// desktop.
    pub project_tools: Option<Arc<dyn ProjectTools>>,
    /// This device's team link: for its own desktops' joins, and the team
    /// tools of the mesh's sessions.
    pub team: Option<TeamLink>,
    /// The mesh this device is in.
    pub mesh: Option<Mesh>,
}

impl Caller {
    /// This node's own agent calling it.
    pub(crate) fn local(offers: &Offers) -> Self {
        Self {
            table: Table::Shared,
            desktop_here: false,
            commands_off: None,
            transfers: offers.transfers.get(),
            agents: Some(offers.agents.clone()),
            project_tools: offers.project_tools.clone(),
            team: offers.reach.peers().map(|peers| peers.team()),
            mesh: offers.mesh.get(),
        }
    }
}
