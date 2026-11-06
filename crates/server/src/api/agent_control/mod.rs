//! Starting, reading and steering coding-agent sessions from released apps. The server reaches
//! no machine: hosts are empty and actions are refused; a desktop runs them over its mesh.

mod pictures;
pub(super) use pictures::{get_agent_image, get_agent_thumb};

use super::{Reply, own_nodes::retired};
use crate::{api::router::Cx, protocol::*};

pub(super) async fn list_agent_hosts(_: Cx, _: ListAgentHosts) -> Reply<Vec<AgentHost>> {
    Ok(Vec::new())
}

pub(super) async fn resume_agent_session(_: Cx, _: ResumeAgentSession) -> Reply<StartedAgent> {
    retired()
}

pub(super) async fn start_agent_session(_: Cx, _: StartAgentSession) -> Reply<StartedAgent> {
    retired()
}

pub(super) async fn preview_agent_machine(_: Cx, _: PreviewAgentMachine) -> Reply<MachinePick> {
    retired()
}

pub(super) async fn get_agent_session(_: Cx, _: GetAgentSession) -> Reply<AgentSessionView> {
    retired()
}

pub(super) async fn get_agent_session_usage(
    _: Cx,
    _: GetAgentSessionUsage,
) -> Reply<AgentSessionUsage> {
    retired()
}

pub(super) async fn get_agent_session_modes(
    _: Cx,
    _: GetAgentSessionModes,
) -> Reply<AgentSessionModes> {
    retired()
}

pub(super) async fn get_agent_session_tasks(
    _: Cx,
    _: GetAgentSessionTasks,
) -> Reply<AgentSessionTasks> {
    retired()
}

pub(super) async fn steer_agent_session(_: Cx, _: SteerAgentSession) -> Reply<()> {
    retired()
}

/// The apps read each picture by key with `GetAgentImage`; there is nothing to list.
pub(super) async fn list_agent_images(_: Cx, _: ListAgentImages) -> Reply<Vec<AgentImage>> {
    Ok(Vec::new())
}

pub(super) async fn get_agent_file(_: Cx, _: GetAgentFile) -> Reply<AgentFileChunk> {
    retired()
}

pub(super) async fn list_agent_models(_: Cx, _: ListAgentModels) -> Reply<AgentModels> {
    retired()
}
