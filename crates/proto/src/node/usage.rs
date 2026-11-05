//! What coding agents cost and how much of their plans is left, as a node
//! reports it on a `SessionsRequest::FollowUsage` stream.
//!
//! Like the rest of the node link these are compact msgpack, so a field is
//! added only at the end with `#[serde(default)]`.

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
    /// The agent's own id of the session, such as Claude Code's session
    /// id, when it said: the key a `SessionSpend` of the same session
    /// has, so the server counts the session once.
    pub agent_session: Option<String>,
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

/// The most hour buckets one `SessionSpend` carries.
pub const MAX_SESSION_HOURS: usize = 4_000;

/// What one session of a coding agent spent, as the node read it from the
/// agent's own session log, sessions started outside Beep included. Sent
/// on a `FollowUsage` stream that asked for `history`. Each report is whole
/// from `since` on: it replaces what the server kept for the session from
/// that hour, so reading a log again never counts it twice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionSpend {
    /// The agent, such as `claude`.
    pub agent: AgentSlug,
    /// The agent's own id of the session, as `TurnUsage::agent_session`
    /// names it.
    pub session: String,
    /// The folder it ran in.
    pub cwd: String,
    /// The start of what the report covers, in Unix seconds.
    pub since: u64,
    /// What it spent each hour with each model.
    pub hours: Vec<HourSpend>,
}

/// What one session spent with one model in one hour.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HourSpend {
    /// The start of the hour, in Unix seconds.
    pub hour: u64,
    /// The model as the agent names it, such as `claude-opus-4-1`.
    pub model: ModelId,
    /// Input tokens read fresh, not from the cache.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// What the priced tokens cost at API prices, in US dollars.
    pub cost_usd: f64,
    /// The tokens no price covered, of a model the node has no price for.
    pub unpriced_tokens: u64,
    /// How it was paid for. `Unknown` from a node from before it said.
    pub billing: Billing,
}

/// How what an agent spent was paid for.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Billing {
    /// A subscription covered it, such as a Claude or ChatGPT plan or
    /// GitHub Copilot: the tokens cost nothing more.
    Subscription,
    /// Paid by the token: an API key, OpenRouter, Bedrock, Vertex.
    Metered,
    /// The node could not tell, or a newer node named a way this one does
    /// not know.
    #[default]
    #[serde(other)]
    Unknown,
}

impl Billing {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Subscription => "subscription",
            Self::Metered => "metered",
        }
    }

    /// The billing `as_str` names; `Unknown` for any other name.
    pub fn from_name(name: &str) -> Self {
        match name {
            "subscription" => Self::Subscription,
            "metered" => Self::Metered,
            _ => Self::Unknown,
        }
    }
}

impl HourSpend {
    /// Every token: input, cached input, cache writes and output.
    pub fn tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }
}

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
    fn session_spend_round_trips_and_an_older_turn_reads() {
        let spend = SessionSpend {
            agent: "claude".into(),
            session: "0f3c2a9e-1111-4222-8333-944455556666".into(),
            cwd: "/srv/project".into(),
            since: 1_789_000_000,
            hours: vec![HourSpend {
                hour: 1_789_995_600,
                model: "claude-opus-4-1".into(),
                input_tokens: 10,
                output_tokens: 20,
                cache_read_tokens: 300,
                cache_write_tokens: 40,
                cost_usd: 0.5,
                unpriced_tokens: 0,
                billing: Billing::Metered,
            }],
        };
        let body = rmp_serde::to_vec_named(&spend).unwrap();
        assert_eq!(rmp_serde::from_slice::<SessionSpend>(&body).unwrap(), spend);
        assert_eq!(spend.hours[0].tokens(), 370);

        #[derive(Serialize)]
        struct OlderTurn {
            session: &'static str,
            tokens: u64,
        }
        let body = rmp_serde::to_vec_named(&OlderTurn {
            session: "a3f9c1",
            tokens: 5,
        })
        .unwrap();
        let read: TurnUsage = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.tokens, 5);
        assert_eq!(read.agent_session, None);
    }

    #[test]
    fn an_older_hour_and_a_newer_billing_read_as_unknown() {
        #[derive(Serialize)]
        struct OlderHour {
            hour: u64,
            cost_usd: f64,
        }
        let body = rmp_serde::to_vec_named(&OlderHour {
            hour: 3_600,
            cost_usd: 1.0,
        })
        .unwrap();
        let read: HourSpend = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.billing, Billing::Unknown);

        #[derive(Serialize)]
        struct NewerHour {
            hour: u64,
            billing: &'static str,
        }
        let body = rmp_serde::to_vec_named(&NewerHour {
            hour: 3_600,
            billing: "prepaid",
        })
        .unwrap();
        let read: HourSpend = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.billing, Billing::Unknown);
        assert_eq!(
            Billing::from_name(Billing::Metered.as_str()),
            Billing::Metered
        );
    }

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
