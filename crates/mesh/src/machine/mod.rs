//! What a desktop or the server asks of one node, and how the node's
//! answers read to an app: the typed requests, the checks on what an app
//! may send, and the conversions to the apps' own reply types. Both callers
//! share it; neither keeps its own copy.

pub mod agent;
pub mod agent_files;
pub mod auto;
pub mod codex_login;
pub mod listing;
mod node;
pub mod releases;
pub mod sessions;
pub mod terminals;
pub mod thumb;

pub use agent::{
    Makes, Opened, StartOn, checkout_for, ended, node_request, open_agent, order, pick_agent,
    picked_effort, picked_model, queued_instead,
};
pub use listing::{
    Host, agent_models, beep_session, beep_view, entries_for, external_session, machine_capability,
    past_session, session_tasks,
};
pub use node::{
    Exec, Tasks, Updated, exec, files, install_tool, log, rescan_search, search, set_search,
    update_node,
};
pub use sessions::{NodeFile, NodeImage, NodeWork, Sessions, WorkRead, WorkWrite};

use crate::MeshError;
use anyhow::anyhow;

/// What the node said in place of an answer, as the error a caller shows.
pub(crate) fn said(message: impl Into<String>) -> MeshError {
    MeshError::Link(anyhow!(message.into()))
}

/// The node answered with something this request never gets.
pub(crate) fn unexpected(reply: &impl std::fmt::Debug) -> MeshError {
    MeshError::Unexpected(format!("{reply:?}"))
}
