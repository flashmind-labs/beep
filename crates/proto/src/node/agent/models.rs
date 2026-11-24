//! Which model a session runs on and how hard it thinks.

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
#[derive(strum::AsRefStr, strum::EnumString)]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    None,
    Ultra,
}

/// The thinking levels the agent `agent` takes at start; none for an agent
/// whose thinking cannot be set from here. Each mirrors its CLI, whose
/// ACP adapter offers the same levels as a `thought_level` option:
///
/// - `claude`: `claude --effort` (low, medium, high, xhigh, max).
/// - `codex`: `codex -c model_reasoning_effort=` (none, minimal, low, medium, high, xhigh, max, ultra).
/// - `agy`: `agy --effort` (low, medium, high, xhigh, max).
/// - `pi`: `pi --thinking` (off, minimal, low, medium, high, xhigh, max).
/// - `flsh`: ACP `thought_level` (off, low, medium, high), from 0.2.7.
///
/// OpenCode has no such setting.
pub fn efforts(agent: &str) -> &'static [Effort] {
    use Effort::*;

    match agent {
        "claude" | "agy" => &[Low, Medium, High, Xhigh, Max],
        "codex" => &[None, Minimal, Low, Medium, High, Xhigh, Max, Ultra],
        "pi" => &[Off, Minimal, Low, Medium, High, Xhigh, Max],
        "flsh" => &[Off, Low, Medium, High],
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
    /// The harness's default usable context, when reported.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// The largest usable window the harness reports for this model.
    #[serde(default)]
    pub max_context_window: Option<u64>,
    /// Who serves it, such as `openrouter`, when the id names one; the
    /// name leaves it out.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub efforts: Vec<Effort>,
    #[serde(default)]
    pub fast: Option<bool>,
}

#[cfg(test)]
#[path = "../../tests/node_agent_models.rs"]
mod tests;
