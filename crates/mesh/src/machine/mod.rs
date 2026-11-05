//! What a desktop or the server asks of one node, and how the node's
//! answers read to an app: the typed requests, the checks on what an app
//! may send, and the conversions to the apps' own reply types. Both callers
//! share it; neither keeps its own copy.

pub mod agent;
pub mod agent_files;
pub mod auto;
pub mod codex_login;
mod convert;
pub mod listing;
mod node;
pub mod releases;
pub mod sessions;
mod steer;
pub mod terminals;
pub mod thumb;

pub use agent::{Opened, StartOn, open_agent, order};
pub use convert::{
    Host, agent_models, beep_session, beep_view, entries_for, external_session, machine_capability,
    past_session, runs_now, session_tasks,
};
pub use node::{
    Exec, Tasks, Updated, exec, files, install_tool, log, search, set_search, task_request,
    update_node,
};
pub use sessions::{NodeFile, NodeImage, NodeWork, Sessions, WorkRead, WorkWrite};
pub use steer::{
    Makes, checkout_for, node_request, pick_agent, picked_effort, picked_model, queued_instead,
};

use crate::MeshError;
use anyhow::anyhow;
pub use beep_proto::agent_name;

/// What the node said in place of an answer, as the error a caller shows.
pub(crate) fn said(message: impl Into<String>) -> MeshError {
    MeshError::Link(anyhow!(message.into()))
}

/// The node answered with something this request never gets.
pub(crate) fn unexpected(reply: &impl std::fmt::Debug) -> MeshError {
    MeshError::Unexpected(format!("{reply:?}"))
}
