//! Agents: a system prompt, a model and a thinking level tuned for one kind
//! of task. When an admin turns agents on, Jev picks one for every prompt,
//! for every user. General is today's assistant and the fallback.

use flashmind::types::{Model, ReasoningLevel};
use serde::Deserialize;
use std::fmt::Write;
use strum::{EnumString, IntoStaticStr};

/// The agents the server knows. Each has its own prompt, compiled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, EnumString, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AgentId {
    General,
    GettingAround,
    Shopping,
    GoingOut,
    Research,
    Inbox,
}

impl AgentId {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// The full system prompt the agent's turns run with.
    pub fn prompt(self) -> &'static str {
        match self {
            Self::General => include_str!("../prompts/assistant.md"),
            Self::GettingAround => include_str!("../prompts/agents/getting_around.md"),
            Self::Shopping => include_str!("../prompts/agents/shopping.md"),
            Self::GoingOut => include_str!("../prompts/agents/going_out.md"),
            Self::Research => include_str!("../prompts/agents/research.md"),
            Self::Inbox => include_str!("../prompts/agents/inbox.md"),
        }
    }
}

/// What Jev reads to route a prompt: the chat's current agent, the turn
/// before the prompt, and the prompt itself.
pub fn router_state(
    current: Option<AgentId>,
    earlier: Option<&str>,
    message: &str,
    images: usize,
) -> String {
    let mut state = String::new();
    if let Some(agent) = current {
        let _ = write!(state, "Current agent: {}\n\n", agent.as_str());
    }
    if let Some(turn) = earlier {
        let _ = write!(state, "Earlier turn:\n{turn}\n\n");
    }
    state.push_str("Latest message from the user: ");
    state.push_str(message);
    if images > 0 {
        let _ = write!(state, "\n({images} image(s) attached)");
    }
    state
}

/// An agent as the config describes it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// What the app shows, like "Getting around".
    pub name: String,
    /// What it handles, as Jev reads it to choose.
    pub when: String,
    /// Must be one of the server's model choices.
    pub model: Model,
    /// The least it thinks; Jev may raise it one step.
    pub thinking: ReasoningLevel,
}

/// An agent whose model the server offers.
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    pub when: String,
    pub model: Model,
    pub thinking: ReasoningLevel,
}

impl Agent {
    /// The thinking for a turn Jev judged to need `wanted`: the agent's own
    /// level, raised by at most one step.
    pub fn thinking_for(&self, wanted: ReasoningLevel) -> ReasoningLevel {
        let floor = rank(self.thinking);
        let level = rank(wanted).clamp(floor, (floor + 1).min(3));
        LEVELS[level]
    }
}

const LEVELS: [ReasoningLevel; 4] = [
    ReasoningLevel::Off,
    ReasoningLevel::Low,
    ReasoningLevel::Medium,
    ReasoningLevel::High,
];

fn rank(level: ReasoningLevel) -> usize {
    LEVELS.iter().position(|&l| l == level).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(thinking: ReasoningLevel) -> Agent {
        Agent {
            id: AgentId::General,
            name: "General".into(),
            when: String::new(),
            model: "openrouter:a/b".parse().expect("a valid model id"),
            thinking,
        }
    }

    #[test]
    fn thinking_rises_one_step_at_most() {
        let low = agent(ReasoningLevel::Low);
        assert_eq!(low.thinking_for(ReasoningLevel::Off), ReasoningLevel::Low);
        assert_eq!(
            low.thinking_for(ReasoningLevel::Medium),
            ReasoningLevel::Medium
        );
        assert_eq!(
            low.thinking_for(ReasoningLevel::High),
            ReasoningLevel::Medium
        );
        let high = agent(ReasoningLevel::High);
        assert_eq!(high.thinking_for(ReasoningLevel::Low), ReasoningLevel::High);
    }

    #[test]
    fn ids_read_as_config_keys() {
        assert_eq!("getting_around".parse(), Ok(AgentId::GettingAround));
        assert_eq!(AgentId::Inbox.as_str(), "inbox");
    }
}
