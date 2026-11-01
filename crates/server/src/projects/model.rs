//! Types shared by the project modules.

use beep_proto::node::agent;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

/// The coding agent a worker machine runs for a project.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub(crate) enum WorkerAgent {
    Claude,
    Codex,
}

impl WorkerAgent {
    /// The protocol name a node serves this agent on.
    pub(crate) fn protocol(self) -> String {
        agent::protocol(self.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_agent_names_round_trip() {
        assert_eq!("claude".parse(), Ok(WorkerAgent::Claude));
        assert_eq!(WorkerAgent::Codex.to_string(), "codex");
        assert_eq!(WorkerAgent::Codex.protocol(), agent::protocol("codex"));
    }
}
