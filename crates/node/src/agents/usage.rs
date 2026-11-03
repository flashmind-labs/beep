//! What the node's coding agents spent and how much of their plans is
//! left, waiting for the server's `FollowUsage` stream.
//!
//! Each finished turn waits in a bounded outbox. Plan usage is kept as the
//! latest report per agent: `plans` reads it from the agents' own CLIs,
//! and a running Claude Code session updates it as its rate limit events
//! come.

use beep_proto::{
    AgentSlug,
    node::usage::{LimitWindow, PlanUsage, TurnUsage},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, MutexGuard, PoisonError},
};
use tokio::sync::{Notify, watch};

/// The most turns waiting for the server; the oldest go.
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
    fn turns(&self) -> MutexGuard<'_, VecDeque<TurnUsage>> {
        self.turns.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Has `turn` wait for the server.
    pub(crate) fn record(&self, turn: TurnUsage) {
        let mut turns = self.turns();
        turns.push_back(turn);
        while turns.len() > MAX_UNSENT {
            turns.pop_front();
        }
        drop(turns);
        self.arrived.notify_one();
    }

    /// The oldest turn waiting for the server, once there is one.
    pub(crate) async fn next_turn(&self) -> TurnUsage {
        loop {
            let arrived = self.arrived.notified();
            if let Some(turn) = self.turns().pop_front() {
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
        self.plans.send_if_modified(|plans| {
            let changed = plans.get(&plan.agent) != Some(&plan);
            plans.insert(plan.agent.clone(), plan);
            changed
        });
    }

    /// Puts `windows`, seen `at` (Unix seconds), into `agent`'s report,
    /// replacing the same windows and keeping the others.
    pub(crate) fn update_windows(&self, agent: &AgentSlug, windows: Vec<LimitWindow>, at: u64) {
        if windows.is_empty() {
            return;
        }
        self.plans.send_if_modified(|plans| {
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
    }
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

/// How long a Claude Code limit window is, in minutes, and what it is
/// limited to, by its name; `None` for one that is no plan window, such
/// as overage.
pub(crate) fn claude_window(kind: &str) -> Option<(u32, Option<&'static str>)> {
    const WEEK: u32 = 7 * 24 * 60;
    Some(match kind {
        "five_hour" => (5 * 60, None),
        "seven_day" => (WEEK, None),
        "seven_day_opus" => (WEEK, Some("Opus")),
        "seven_day_sonnet" => (WEEK, Some("Sonnet")),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_rate_limit_event_updates_its_windows_only() {
        let hub = UsageHub::default();
        hub.set_plan(PlanUsage {
            agent: AgentSlug::CLAUDE,
            plan: Some("max".into()),
            windows: vec![
                LimitWindow {
                    minutes: 300,
                    used_percent: 10.0,
                    resets_at: Some(100),
                    scope: None,
                },
                LimitWindow {
                    minutes: 10080,
                    used_percent: 1.0,
                    resets_at: Some(200),
                    scope: None,
                },
            ],
            at: 50,
        });
        let event = json!({
            "status": "allowed",
            "rateLimitType": "seven_day",
            "unifiedWindows": {"seven_day": {"utilization": 0.02, "resetsAt": 300, "status": "allowed"}},
        });
        hub.update_windows(&AgentSlug::CLAUDE, claude_rate_limit(&event), 60);
        let plans = hub.plans();
        let plan = &plans.borrow()["claude"];
        assert_eq!(plan.plan.as_deref(), Some("max"));
        assert_eq!(plan.at, 60);
        assert_eq!(plan.windows[0].used_percent, 10.0);
        assert_eq!(plan.windows[1].used_percent, 2.0);
        assert_eq!(plan.windows[1].resets_at, Some(300));
    }

    #[test]
    fn an_older_event_gives_its_limiting_window() {
        let event = json!({"status": "allowed_warning", "rateLimitType": "five_hour", "utilization": 0.8, "resetsAt": 9});
        let windows = claude_rate_limit(&event);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].minutes, 300);
        assert_eq!(windows[0].used_percent, 80.0);
        assert!(claude_rate_limit(&json!({"status": "allowed"})).is_empty());
    }

    #[tokio::test]
    async fn turns_wait_bounded_for_the_server() {
        let hub = UsageHub::default();
        for n in 0..(MAX_UNSENT as u64 + 3) {
            hub.record(TurnUsage {
                tokens: n,
                ..TurnUsage::default()
            });
        }
        assert_eq!(hub.next_turn().await.tokens, 3);
    }
}
