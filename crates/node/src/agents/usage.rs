//! What the node's coding agents spent and how much of their plans is left
//! (`UsageHub`), for the callers' `FollowUsage` streams: each follower has
//! its own place among the finished turns. Plan usage is the
//! latest report per agent, from `plans` or Claude Code's rate limit events.

use crate::agents::AlertHub;
use beep_proto::{
    AccountName, AgentSlug, ProviderCredits,
    node::usage::{LimitWindow, PlanUsage, TurnUsage},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

/// The most finished turns kept for the followers; the oldest go.
const MAX_TURNS: usize = 500;

/// Whose plan a report is: an agent, and its named account or `None` for
/// its default one.
pub(crate) type PlanOf = (AgentSlug, Option<AccountName>);

/// The plan usage of each agent's account.
pub(crate) type Plans = BTreeMap<PlanOf, PlanUsage>;

#[derive(Debug)]
pub(crate) struct UsageHub {
    /// The finished turns, each follower reading them at its own pace.
    pub(crate) turns: AlertHub<TurnUsage>,
    plans: watch::Sender<Plans>,
    /// The one timer that reads the plans, alive while a stream holds it.
    timer: Mutex<Weak<AbortOnDropHandle<()>>>,
}

/// `of`'s entry in `plans`, empty when there is none.
fn plan_of<'a>(plans: &'a mut Plans, of: &PlanOf) -> &'a mut PlanUsage {
    plans.entry(of.clone()).or_insert_with(|| PlanUsage {
        agent: of.0.clone(),
        account: of.1.clone(),
        ..PlanUsage::default()
    })
}

/// Tells the event followers that usage changed.
fn usage_changed() {
    crate::events::system().changed(beep_proto::node::events::Topic::new("usage"));
}

impl Default for UsageHub {
    fn default() -> Self {
        Self {
            // A follower that connects late still gets every turn kept.
            turns: AlertHub::keeping(MAX_TURNS, Duration::MAX),
            plans: watch::Sender::new(Plans::new()),
            timer: Mutex::default(),
        }
    }
}

impl UsageHub {
    /// Keeps `turn` for every follower.
    pub(crate) fn record(&self, turn: TurnUsage) {
        self.turns.record(turn);
        usage_changed();
    }

    /// Follows the plans: each agent's latest report.
    pub(crate) fn plans(&self) -> watch::Receiver<Plans> {
        self.plans.subscribe()
    }

    /// The node's timer for reading the plans: `start` runs when no stream
    /// holds one, and the timer ends with the last stream that does.
    pub(crate) fn timer(
        &self,
        start: impl FnOnce() -> AbortOnDropHandle<()>,
    ) -> Arc<AbortOnDropHandle<()>> {
        let mut held = crate::lock(&self.timer);

        held.upgrade().unwrap_or_else(|| {
            let timer = Arc::new(start());
            *held = Arc::downgrade(&timer);
            timer
        })
    }

    /// Takes `plan` in place of the agent's report before.
    pub(crate) fn set_plan(&self, plan: PlanUsage) {
        let changed = self.plans.send_if_modified(|plans| {
            let of = (plan.agent.clone(), plan.account.clone());
            let changed = plans.get(&of) != Some(&plan);
            plans.insert(of, plan);
            changed
        });

        if changed {
            usage_changed();
        }
    }

    /// Replaces a provider's last balance without changing session spend or plan windows.
    pub(crate) fn update_credits(&self, of: &PlanOf, credits: ProviderCredits, at: u64) {
        let changed = self.plans.send_if_modified(|plans| {
            let plan = plan_of(plans, of);
            let before = plan.clone();

            if let Some(kept) = plan
                .credits
                .iter_mut()
                .find(|kept| kept.provider == credits.provider)
            {
                *kept = credits;
            } else {
                plan.credits.push(credits);
            }

            let changed = plan.credits != before.credits;
            if changed {
                plan.at = at;
            }
            changed
        });

        if changed {
            usage_changed();
        }
    }

    /// Puts `windows`, seen `at` (Unix seconds), into `of`'s report,
    /// replacing the same windows and keeping the others.
    pub(crate) fn update_windows(&self, of: &PlanOf, windows: Vec<LimitWindow>, at: u64) {
        if windows.is_empty() {
            return;
        }

        let changed = self.plans.send_if_modified(|plans| {
            let plan = plan_of(plans, of);
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
            usage_changed();
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
