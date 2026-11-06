//! Coding-agent sessions on the user's machines: a desktop reads them over its mesh now.

use super::{Reply, own_nodes::retired};
use crate::{api::router::Cx, protocol::*};

pub(super) async fn list_agent_sessions(_: Cx, _: ListAgentSessions) -> Reply<Vec<MachineSession>> {
    Ok(Vec::new())
}

pub(super) async fn answer_agent_question(_: Cx, _: AnswerAgentQuestion) -> Reply<()> {
    retired()
}
