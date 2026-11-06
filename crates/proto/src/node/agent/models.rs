//! Which model a session runs on and how hard it thinks.

use crate::AgentEffort;
use serde::{Deserialize, Serialize};

/// The models an agent can run on a node, as `SessionsRequest::Models`
/// finds them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelList {
    pub pick: ModelPick,
    /// The model the agent runs when none is picked, when it says.
    pub current: Option<String>,
    /// The models to pick from, for `ModelPick::Listed`.
    pub models: Vec<ModelOption>,
}

/// The longest model id a start may name.
const MAX_MODEL: usize = 200;

/// How hard a coding agent thinks. Its name is the value each agent's ACP
/// adapter takes on its config option of category `thought_level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[non_exhaustive]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// The value an agent's `thought_level` option takes.
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

impl From<AgentEffort> for Effort {
    fn from(effort: AgentEffort) -> Self {
        match effort {
            AgentEffort::Off {} => Self::Off,
            AgentEffort::Minimal {} => Self::Minimal,
            AgentEffort::Low {} => Self::Low,
            AgentEffort::Medium {} => Self::Medium,
            AgentEffort::High {} => Self::High,
            AgentEffort::Xhigh {} => Self::Xhigh,
            AgentEffort::Max {} => Self::Max,
        }
    }
}

impl From<Effort> for AgentEffort {
    fn from(effort: Effort) -> Self {
        match effort {
            Effort::Off => Self::Off {},
            Effort::Minimal => Self::Minimal {},
            Effort::Low => Self::Low {},
            Effort::Medium => Self::Medium {},
            Effort::High => Self::High {},
            Effort::Xhigh => Self::Xhigh {},
            Effort::Max => Self::Max {},
        }
    }
}

/// The thinking levels the agent `agent` takes at start; none for an agent
/// whose thinking cannot be set from here. Each mirrors its CLI, whose
/// ACP adapter offers the same levels as a `thought_level` option:
///
/// - `claude`: `claude --effort` (low, medium, high, xhigh, max).
/// - `codex`: `codex -c model_reasoning_effort=` (low, medium, high).
/// - `pi`: `pi --thinking` (off, minimal, low, medium, high, xhigh, max).
///
/// Gemini CLI, flsh and OpenCode have no such flag: flsh keeps it in its
/// own config.
pub fn efforts(agent: &str) -> &'static [Effort] {
    use Effort::*;
    match agent {
        "claude" => &[Low, Medium, High, Xhigh, Max],
        "codex" => &[Low, Medium, High],
        "pi" => &[Off, Minimal, Low, Medium, High, Xhigh, Max],
        _ => &[],
    }
}

/// Whether `model` may name a model to start an agent on: short, and no
/// control characters, since it may go on the agent's command line.
pub fn valid_model(model: &str) -> bool {
    !model.is_empty() && model.len() <= MAX_MODEL && !model.chars().any(char::is_control)
}

/// How a model is picked for an agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ModelPick {
    /// One of `ModelList::models`.
    Listed,
    /// Any name the agent takes; it lists none.
    Named,
    /// None can be picked: the agent runs its own.
    #[default]
    #[serde(other)]
    Fixed,
}

/// One model an agent offers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelOption {
    /// What `Start`'s `model` names it by.
    pub id: String,
    /// What people call it; the id when the agent gives no name.
    pub name: String,
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_short_and_plain() {
        assert!(valid_model("openrouter:anthropic/claude-sonnet-4"));
        assert!(valid_model("gpt-5.1-codex[high]"));
        assert!(!valid_model(""));
        assert!(!valid_model("a\nb"));
        assert!(!valid_model(&"m".repeat(MAX_MODEL + 1)));
    }
}
