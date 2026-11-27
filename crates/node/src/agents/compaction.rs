//! How a session gets compacted. An agent with its own auto-compaction is
//! configured through its environment (`Policy::env`). Only an agent with no
//! way to configure its own gets the node's `Compactor`: once a turn ends past
//! the threshold `Policy` sets, it sends `/compact` at once, ahead of any
//! prompt waiting.
//! A token count instead of a percent goes to Codex as its own limit, and to
//! Claude Code and pi as the node's `/compact` (Claude Code's own trigger is
//! then turned off).
//! Antigravity takes a percent or a token count in one variable. Agents that
//! compact on their own with no setting (flsh) are left to it.

use beep_proto::{
    AgentContext, ModelId,
    node::{
        agent::AgentCommand,
        harness::{AutoCompact, ModelCompact},
    },
};
pub(super) use crate::known::Route;
use std::collections::BTreeMap;

pub(super) use crate::logs::window::{COMPACTED_ENTRY, MAX_SUMMARY, summary_note};

/// The prompt that asks an agent to compact its conversation.
pub(super) const COMPACT_PROMPT: &str = "/compact";
pub(super) const CLAUDE_CONTINUATION: &str =
    "This session is being continued from a previous conversation";

/// Whether and when the node compacts a harness's sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Policy {
    Off,
    /// At this percent of the window, 1 to 100.
    At(u8),
    /// At this many tokens in the conversation, whatever the window.
    AtTokens(u64),
}

impl From<Option<AutoCompact>> for Policy {
    /// The policy a harness's setting gives: the default when it sets
    /// none or a percent out of range.
    fn from(setting: Option<AutoCompact>) -> Self {
        let Some(setting) = setting else {
            return Self::default();
        };

        if !setting.enabled {
            return Self::Off;
        }

        if let Some(tokens) = setting.tokens {
            if tokens > 0 {
                return Self::AtTokens(tokens);
            }

            log::warn!("auto-compact at 0 tokens is not a limit; using the percent");
        }

        match setting.percent {
            None => Self::default(),
            Some(percent) if AutoCompact::valid_percent(percent) => Self::At(percent),
            Some(percent) => {
                log::warn!(
                    "auto-compact {percent}% is not 1 to 100; compacting at {}%",
                    AutoCompact::DEFAULT_PERCENT
                );
                Self::default()
            }
        }
    }
}

impl Policy {
    /// Whether a session at `usage` is due a compaction.
    pub(super) fn due(self, usage: Option<AgentContext>) -> bool {
        let Some(usage) = usage else {
            return false;
        };
        let percent = match self {
            Self::Off => return false,
            Self::AtTokens(tokens) => return usage.used >= tokens,
            Self::At(percent) => percent,
        };

        if usage.size == 0 {
            return false;
        }
        // Widened, so neither side can overflow.
        u128::from(usage.used) * 100 >= u128::from(usage.size) * u128::from(percent)
    }

    /// The variable to add so the agent's own trigger matches, when `route`
    /// has one and the user has not set it.
    pub(super) fn env(self, route: Route, user: &BTreeMap<String, String>) -> Option<EnvVar> {
        let (name, value) = match (route, self) {
            (Route::Percent { percent_var, .. }, Self::At(percent)) => {
                (percent_var, percent.to_string())
            }
            // The node compacts by token count, so the agent's own trigger
            // would only get in its way.
            (Route::Percent { off_var, .. } | Route::Toggle { off_var }, Self::Off)
            | (Route::Percent { off_var, .. }, Self::AtTokens(_)) => (off_var, "1".to_owned()),
            (Route::Threshold { var }, Self::At(percent)) => (var, format!("{percent}%")),
            (Route::Threshold { var }, Self::AtTokens(tokens)) => (var, tokens.to_string()),
            (Route::TokenLimit { config_var, key }, Self::AtTokens(tokens)) => {
                (config_var, serde_json::json!({ key: tokens }).to_string())
            }
            _ => return None,
        };
        (!user.contains_key(name)).then_some(EnvVar { name, value })
    }

    /// The variable that would turn the agent's own trigger off, to drop
    /// from what the node inherited when this policy wants it on and the
    /// user has not set it.
    pub(super) fn cleared(self, route: Route, user: &BTreeMap<String, String>) -> Option<&'static str> {
        let name = match (route, self) {
            (Route::Percent { off_var, .. } | Route::Toggle { off_var }, Self::At(_)) => off_var,
            _ => return None,
        };
        (!user.contains_key(name)).then_some(name)
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::At(AutoCompact::DEFAULT_PERCENT)
    }
}

/// A variable to set on an agent's process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EnvVar {
    pub(super) name: &'static str,
    pub(super) value: String,
}



impl Route {
    /// Whether the node sends `/compact` under `policy`, given the commands
    /// the agent advertised.
    pub(super) fn takes_compact(self, policy: Policy, commands: &[AgentCommand]) -> bool {
        let tokens = matches!(policy, Policy::AtTokens(_));
        let node_compacts = match self {
            Self::Advertised => true,
            // Its own setting has no token count, so the node counts.
            Self::Percent { .. } => tokens,
            Self::TokenLimit { .. } => !tokens,
            Self::OwnTrigger | Self::Toggle { .. } | Self::Threshold { .. } => false,
        };
        node_compacts
            && commands
                .iter()
                .any(|command| command.name.trim_start_matches('/') == "compact")
    }
}

/// The node's compaction state for one session.
#[derive(Debug)]
pub(super) struct Compactor {
    policy: Policy,
    route: Route,
    /// A turn ended past the threshold; `/compact` goes before any other
    /// prompt.
    due: bool,
    /// The node's `/compact` is running.
    running: bool,
    /// What the agent said while it ran: its summary.
    reply: String,
    models: Vec<ModelCompact>,
    model: Option<ModelId>,
}

/// What a session looks like when its turn ends.
#[derive(Debug, Clone, Copy)]
pub(super) struct TurnEnd<'a> {
    pub(super) usage: Option<AgentContext>,
    pub(super) commands: &'a [AgentCommand],
}

impl Compactor {
    pub(super) fn new(route: Route, policy: Policy) -> Self {
        Self {
            policy,
            route,
            due: false,
            running: false,
            reply: String::new(),
            models: Vec::new(),
            model: None,
        }
    }

    pub(super) fn with_models(mut self, models: Vec<ModelCompact>) -> Self {
        if !models.is_empty() {
            self.route = Route::Advertised;
        }
        self.models = models;
        self
    }

    pub(super) fn set_model(&mut self, model: Option<&str>) -> bool {
        if self.model.as_ref().map(ModelId::as_ref) == model {
            return false;
        }

        self.model = model.map(ModelId::from);
        self.due = false;
        true
    }

    /// A turn the user asked for ended.
    pub(super) fn turn_ended(&mut self, end: TurnEnd<'_>) {
        let override_policy = self
            .models
            .iter()
            .find(|rule| Some(rule.model.as_ref()) == self.model.as_ref().map(ModelId::as_ref))
            .map(|rule| Policy::from(Some(rule.compact)));
        let (policy, route) = override_policy.map_or((self.policy, self.route), |policy| {
            (policy, Route::Advertised)
        });
        self.due = policy.due(end.usage) && route.takes_compact(policy, end.commands);
    }

    /// Whether to send `/compact` now, before any other prompt: never while
    /// a question waits or a compaction already runs. True marks it sent.
    pub(super) fn start(&mut self, question_open: bool) -> bool {
        if !self.due || self.running || question_open {
            return false;
        }

        self.due = false;
        self.running = true;
        true
    }

    pub(super) fn running(&self) -> bool {
        self.running
    }

    /// Keeps agent text that answers the node's `/compact`, so it shows as
    /// the compaction and not as part of the last reply. False when none
    /// runs.
    pub(super) fn heard(&mut self, text: &str) -> bool {
        if self.running {
            crate::text::push_cut(&mut self.reply, text, MAX_SUMMARY);
        }
        self.running
    }

    /// The node's `/compact` finished, however it went. Returns what the
    /// agent said meanwhile.
    pub(super) fn finished(&mut self) -> String {
        self.running = false;
        std::mem::take(&mut self.reply)
    }
}

#[cfg(test)]
#[path = "../tests/agents_compaction_model_tests.rs"]
mod model_tests;
#[cfg(test)]
#[path = "../tests/agents_compaction.rs"]
mod tests;
