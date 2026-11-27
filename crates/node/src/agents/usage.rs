//! What the node's coding agents spent and how much of their plans is left
//! (`UsageHub`), for the callers' `FollowUsage` streams: each follower has
//! its own place among the finished turns. Plan usage is the
//! latest report per agent, from `plans` or Claude Code's rate limit events.

use crate::agents::{
    AlertHub,
    catalog::Catalog,
    plans::{self, claude_window},
    spend::Spend,
};
use crate::process::spawn_env;
use anyhow::Result;
use beep_proto::AgentAccount;
use beep_proto::{
    AccountName, AgentSlug, ProviderCredits,
    node::usage::{LimitWindow, PlanUsage, TurnUsage},
};
use futures::{StreamExt, stream::FuturesUnordered};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::watch, time::Instant};
use tokio_util::task::AbortOnDropHandle;

/// What sessions spend and how much of the agents' plans is left, with the
/// reads of the plans.
#[derive(Default)]
pub(crate) struct Usage {
    /// What finished turns spent and the plans' state, for the followers.
    pub(super) hub: Arc<UsageHub>,
    /// What sessions spent, from the agents' own logs.
    pub(super) spend: Arc<Spend>,
    /// Which agents the plans are read from, and how they sign in.
    catalog: Arc<Catalog>,
    /// Set while the agents' plans are read.
    reading_plans: AtomicBool,
    /// When the last plan read started, so short-lived streams do not re-run it before `PLAN_EVERY`.
    last_plan_read: Mutex<Option<Instant>>,
}

impl Usage {
    pub(crate) fn new(catalog: Arc<Catalog>) -> Self {
        Self {
            spend: Arc::new(Spend::locate()),
            catalog,
            ..Self::default()
        }
    }

    /// Starts reading the plans into the hub, unless a read runs. The read
    /// outlives the stream that asked: a desktop listens only briefly, and a
    /// CLI that answers after it left still has its answer kept for the next
    /// look. A read ends within `plans::READ_TIMEOUT`.
    pub(crate) fn read_plans_soon(self: &Arc<Self>) {
        let _ = self.read_plans_if_due(Duration::ZERO);
    }

    /// Starts reading the plans when at least `min_gap` passed since the last
    /// read began, and returns how long to wait until the next due read.
    pub(crate) fn read_plans_if_due(self: &Arc<Self>, min_gap: Duration) -> Duration {
        let mut last = crate::lock(&self.last_plan_read);

        if let Some(at) = *last
            && let Some(remaining) = min_gap.checked_sub(at.elapsed())
            && !remaining.is_zero()
        {
            return remaining;
        }

        if self.reading_plans.swap(true, Ordering::Relaxed) {
            return min_gap;
        }

        *last = Some(Instant::now());
        drop(last);

        let usage = self.clone();
        tokio::spawn(async move {
            usage.read_plans().await;
            usage.reading_plans.store(false, Ordering::Relaxed);
        });
        min_gap
    }

    /// Reads the plans once the node starts, so the first desktop to look
    /// finds them read instead of waiting for Claude Code to start. It
    /// waits for the user's agent settings, which say how each CLI signs in,
    /// for `SETTINGS_WAIT` at most.
    pub(crate) async fn read_plans_at_start(self: &Arc<Self>) {
        if !self.catalog.settings_came().await {
            log::debug!("reading the plans without the agent settings");
        }

        self.read_plans_soon();
    }

    /// Reads how much of each plan is left from the CLIs of the agents this
    /// node runs, once per account, into the hub as each answers: a slow CLI
    /// does not hold back the others. One that cannot say is left out.
    async fn read_plans(&self) {
        let reads = [AgentAccount::Claude, AgentAccount::Codex]
            .into_iter()
            .flat_map(|agent| {
                let named = spawn_env::accounts_of(agent).into_iter().map(Some);
                std::iter::once(None)
                    .chain(named)
                    .map(move |account| (agent, account))
            })
            .map(|(agent, account)| self.read_plan(agent, account))
            .collect::<FuturesUnordered<_>>();
        tokio::join!(reads.collect::<Vec<()>>(), self.read_agy_plan());
    }

    /// Reads Antigravity's plan into the hub; it has no named accounts.
    async fn read_agy_plan(&self) {
        let name = AgentSlug::AGY;
        let Some(command) = self.catalog.cli(name.as_ref(), None, plans::AGY_ARGS) else {
            return;
        };

        self.keep_plan(name.as_ref(), None, plans::read_agy(command).await);
    }

    /// Reads `agent`'s plan on `account` into the hub.
    async fn read_plan(&self, agent: AgentAccount, account: Option<AccountName>) {
        let args = match agent {
            AgentAccount::Claude => plans::CLAUDE_ARGS,
            AgentAccount::Codex => plans::CODEX_ARGS,
        };
        let Some(command) = self.catalog.cli(agent.as_ref(), account.as_ref(), args) else {
            return;
        };
        let read = match agent {
            AgentAccount::Claude => plans::read_claude(command).await,
            AgentAccount::Codex => plans::read_codex(command).await,
        };

        self.keep_plan(agent.as_ref(), account, read);
    }

    /// Keeps `name`'s plan read on `account` in `usage`.
    fn keep_plan(&self, name: &str, account: Option<AccountName>, read: Result<Option<PlanUsage>>) {
        match read {
            Ok(Some(plan)) => self.hub.set_plan(PlanUsage { account, ..plan }),
            Ok(None) => {}
            Err(e) => log::info!("{name} plan usage not read: {e:#}"),
        }
    }
}

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

    /// Takes `plan` in place of the agent's report before when its content changed.
    pub(crate) fn set_plan(&self, plan: PlanUsage) {
        let changed = self.plans.send_if_modified(|plans| {
            let of = (plan.agent.clone(), plan.account.clone());
            let changed = plans.get(&of).is_none_or(|before| {
                before.plan != plan.plan
                    || before.windows != plan.windows
                    || before.credits != plan.credits
            });
            if changed {
                plans.insert(of, plan);
            }
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

#[cfg(test)]
#[path = "../tests/agents_usage.rs"]
mod tests;
