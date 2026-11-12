//! What the node's coding agents spent and how much of their plans is left
//! (`UsageHub`), for the caller's `FollowUsage` stream. Plan usage is the
//! latest report per agent, from `plans` or Claude Code's rate limit events.

use crate::lock;
use beep_proto::{
    AgentSlug, ProviderCredits,
    node::usage::{LimitWindow, PlanUsage, TurnUsage},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};
use tokio::sync::{Notify, watch};

/// The most turns waiting for the caller; the oldest go.
const MAX_UNSENT: usize = 500;

/// The plan usage of each agent, by agent.
pub(crate) type Plans = BTreeMap<AgentSlug, PlanUsage>;

#[derive(Debug)]
pub(crate) struct UsageHub {
    turns: Mutex<VecDeque<TurnUsage>>,
    /// Woken when a turn joins `turns`.
    arrived: Notify,
    plans: watch::Sender<Plans>,
}

impl Default for UsageHub {
    fn default() -> Self {
        Self {
            turns: Mutex::default(),
            arrived: Notify::new(),
            plans: watch::Sender::new(Plans::new()),
        }
    }
}

impl UsageHub {
    /// Has `turn` wait for the caller.
    pub(crate) fn record(&self, turn: TurnUsage) {
        let mut turns = lock(&self.turns);
        turns.push_back(turn);

        while turns.len() > MAX_UNSENT {
            turns.pop_front();
        }

        drop(turns);
        self.arrived.notify_one();
        crate::events::system().changed(beep_proto::node::events::Topic::new("usage"));
    }

    /// The oldest turn waiting for the caller, once there is one.
    pub(crate) async fn next_turn(&self) -> TurnUsage {
        loop {
            let arrived = self.arrived.notified();

            if let Some(turn) = lock(&self.turns).pop_front() {
                return turn;
            }

            arrived.await;
        }
    }

    /// Follows the plans: each agent's latest report.
    pub(crate) fn plans(&self) -> watch::Receiver<Plans> {
        self.plans.subscribe()
    }

    /// Takes `plan` in place of the agent's report before.
    pub(crate) fn set_plan(&self, plan: PlanUsage) {
        let changed = self.plans.send_if_modified(|plans| {
            let changed = plans.get(&plan.agent) != Some(&plan);
            plans.insert(plan.agent.clone(), plan);
            changed
        });

        if changed {
            crate::events::system().changed(beep_proto::node::events::Topic::new("usage"));
        }
    }

    /// Replaces a provider's last balance without changing session spend or plan windows.
    pub(crate) fn update_credits(&self, agent: &AgentSlug, credits: ProviderCredits, at: u64) {
        self.plans.send_modify(|plans| {
            let plan = plans.entry(agent.clone()).or_insert_with(|| PlanUsage {
                agent: agent.clone(),
                ..PlanUsage::default()
            });
            if let Some(kept) = plan
                .credits
                .iter_mut()
                .find(|kept| kept.provider == credits.provider)
            {
                *kept = credits;
            } else {
                plan.credits.push(credits);
            }
            plan.at = at;
        });
    }

    /// Puts `windows`, seen `at` (Unix seconds), into `agent`'s report,
    /// replacing the same windows and keeping the others.
    pub(crate) fn update_windows(&self, agent: &AgentSlug, windows: Vec<LimitWindow>, at: u64) {
        if windows.is_empty() {
            return;
        }

        let changed = self.plans.send_if_modified(|plans| {
            let plan = plans.entry(agent.clone()).or_insert_with(|| PlanUsage {
                agent: agent.clone(),
                ..PlanUsage::default()
            });
            let before = plan.clone();

            for window in windows {
                match plan.windows.iter_mut().find(|kept| kept.same(&window)) {
                    Some(kept) => *kept = window,
                    None => plan.windows.push(window),
                }
            }

            plan.windows.sort_by_key(|window| window.minutes);
            let changed = *plan != before;
            if changed {
                plan.at = at;
            }
            changed
        });

        if changed {
            crate::events::system().changed(beep_proto::node::events::Topic::new("usage"));
        }
    }
}

/// flsh reports decimal balances as strings under `_meta["flsh/credits"]`.
/// Reject malformed amounts while preserving unknown amounts and exact precision.
pub(crate) fn flsh_credits(info: &Value) -> Option<ProviderCredits> {
    let provider = info.get("provider")?.as_str()?;

    if provider.is_empty() || provider.len() > 100 {
        return None;
    }

    let balance = info.get("balance")?;
    let text = |key: &str| -> Option<Option<String>> {
        match balance.get(key) {
            Some(Value::String(value)) if value.len() <= 100 => Some(Some(value.clone())),
            Some(Value::Null) | None => Some(None),
            _ => None,
        }
    };
    let amount = |key| -> Option<Option<String>> {
        let value = text(key)?;

        if let Some(value) = &value {
            let number: f64 = value.parse().ok()?;

            if !number.is_finite() {
                return None;
            }
        }

        Some(value)
    };
    Some(ProviderCredits {
        provider: provider.to_owned(),
        scope: serde_json::from_value(balance.get("scope")?.clone()).ok()?,
        unit: serde_json::from_value(balance.get("unit")?.clone()).ok()?,
        remaining: amount("remaining")?,
        total: amount("total")?,
        used: amount("used")?,
        resets: text("resets")?,
    })
}

/// The windows in a Claude Code rate limit event, as claude-agent-acp
/// forwards it in a `usage_update`'s `_meta["_claude/rateLimit"]`:
/// `unifiedWindows` holds every window it tracks, each `utilization` a
/// fraction; older Claude Code only gives the limiting window at the top.
pub(crate) fn claude_rate_limit(info: &Value) -> Vec<LimitWindow> {
    let window = |kind: &str, value: &Value| {
        let minutes = claude_window(kind)?;
        let used = value.get("utilization")?.as_f64()?;
        Some(LimitWindow {
            minutes: minutes.0,
            used_percent: (used * 100.0).clamp(0.0, 100.0),
            resets_at: value.get("resetsAt").and_then(Value::as_u64),
            scope: minutes.1.map(str::to_owned),
        })
    };

    if let Some(windows) = info.get("unifiedWindows").and_then(Value::as_object) {
        return windows
            .iter()
            .filter_map(|(kind, value)| window(kind, value))
            .collect();
    }

    info.get("rateLimitType")
        .and_then(Value::as_str)
        .and_then(|kind| window(kind, info))
        .into_iter()
        .collect()
}

/// A week, in minutes: the length of Claude Code's weekly windows.
pub(crate) const WEEK_MINUTES: u32 = 7 * 24 * 60;

/// How long a Claude Code limit window is, in minutes, and what it is
/// limited to, by its name; `None` for one that is no plan window, such
/// as overage.
pub(crate) fn claude_window(kind: &str) -> Option<(u32, Option<&'static str>)> {
    Some(match kind {
        "five_hour" => (5 * 60, None),
        "seven_day" => (WEEK_MINUTES, None),
        "seven_day_opus" => (WEEK_MINUTES, Some("Opus")),
        "seven_day_sonnet" => (WEEK_MINUTES, Some("Sonnet")),
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../tests/agents_usage.rs"]
mod tests;
