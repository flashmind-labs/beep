//! What coding agents cost and how much of their plans is left, as a node
//! reports it on a `SessionsRequest::FollowUsage` stream.
//!
//! Like the rest of the node link these are msgpack maps, so either side
//! may add a field with `#[serde(default)]`.

use crate::{AgentSlug, ModelId};
use serde::{Deserialize, Serialize};

/// The longest window a plan limit may name, in minutes: a month.
pub const MAX_WINDOW_MINUTES: u32 = 31 * 24 * 60;

/// What one finished turn of a session spent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnUsage {
    /// The node's id of the session, such as `a3f9c1`.
    pub session: String,
    /// The agent, such as `claude`.
    pub agent: AgentSlug,
    /// The folder the session runs in.
    pub cwd: String,
    /// When the turn ended, in Unix seconds.
    pub at: u64,
    /// What the turn cost at API prices, in US dollars, when the agent
    /// reports a cost (an ACP `usage_update`'s running `cost`, counted per
    /// turn). Codex and Gemini report none.
    pub cost_usd: Option<f64>,
    /// Every token the turn used: input, cached input, output.
    pub tokens: u64,
    /// The same by model, when the agent says; they may add up to more
    /// than `tokens`, counting subagents.
    pub models: Vec<ModelTokens>,
}

/// The tokens one model used.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelTokens {
    /// The model as the agent names it, such as `claude-opus-4-1`.
    pub model: ModelId,
    pub tokens: u64,
}

/// The longest model name kept.
pub const MAX_MODEL_NAME: usize = 200;
/// The most models one turn reports.
pub const MAX_TURN_MODELS: usize = 16;

/// How much of one agent's plan is used, as the node last found it. Each
/// report is whole: it replaces the one before for that agent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanUsage {
    /// The agent, `claude` or `codex`.
    pub agent: AgentSlug,
    /// The plan, such as `max` or `pro`, when the agent says: free-form
    /// text the agent reports.
    pub plan: Option<String>,
    pub windows: Vec<LimitWindow>,
    /// When it was read, in Unix seconds.
    pub at: u64,
}

/// One rate limit window of a plan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimitWindow {
    /// How long the window is: 300 for five hours, 10080 for a week.
    pub minutes: u32,
    /// How much of it is used, from 0 to 100.
    pub used_percent: f64,
    /// When it resets, in Unix seconds, when known.
    pub resets_at: Option<u64>,
    /// What it is limited to, such as `Opus`; `None` for the whole plan.
    /// Free-form text the agent reports.
    pub scope: Option<String>,
}

impl LimitWindow {
    /// Whether `other` is the same window, perhaps at another moment.
    pub fn same(&self, other: &LimitWindow) -> bool {
        self.minutes == other.minutes && self.scope == other.scope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_usage_round_trips_and_reads_without_fields() {
        let plan = PlanUsage {
            agent: AgentSlug::CLAUDE,
            plan: Some("max".into()),
            windows: vec![LimitWindow {
                minutes: 300,
                used_percent: 38.0,
                resets_at: Some(1_790_000_000),
                scope: None,
            }],
            at: 1_789_990_000,
        };
        let body = rmp_serde::to_vec_named(&plan).unwrap();
        assert_eq!(rmp_serde::from_slice::<PlanUsage>(&body).unwrap(), plan);

        #[derive(Serialize)]
        struct Older {
            agent: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Older { agent: "codex" }).unwrap();
        let read: PlanUsage = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.agent, "codex");
        assert!(read.windows.is_empty());
    }
}
