//! The pill atop every command center page: how much of each coding
//! agent's plan is used, and what the agents cost today, this week and
//! this month.

use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_proto::{
    AgentSlug, AgentTokens, HarnessBill, HarnessSpend, ModelSpend, PeriodSpend, PlanLimits,
    PlanWindow, ProjectBill, ProjectSpend, SpendPeriod, SpendReport, UsageReport, UsageView,
};
use serde::Serialize;

/// `UsageReport` as the window reads it, times in milliseconds since the
/// Unix epoch.
#[derive(Debug, Serialize)]
pub struct UsageRow {
    limits: Vec<PlanRow>,
    today: PeriodRow,
    week: PeriodRow,
    /// `None` from a server from before `GetUsageReport`.
    month: Option<PeriodRow>,
    /// The dollars are metered API spend only, subscription use apart;
    /// false from a server from before `GetSpendReport`, whose dollars
    /// price every token at API prices.
    metered: bool,
}

#[derive(Debug, Serialize)]
pub struct PeriodRow {
    total: SpendRow,
    tokens: u64,
    projects: Vec<SpendRow>,
    harnesses: Vec<HarnessRow>,
}

#[derive(Debug, Serialize)]
pub struct PlanRow {
    agent: AgentSlug,
    plan: Option<String>,
    machine_name: String,
    as_of: u64,
    windows: Vec<WindowRow>,
}

#[derive(Debug, Serialize)]
pub struct WindowRow {
    minutes: u32,
    used_percent: f64,
    resets_at: Option<u64>,
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SpendRow {
    project: String,
    cost_usd: f64,
    unpriced: Vec<TokensRow>,
    /// The tokens of metered use; 0 from an older server.
    metered_tokens: u64,
    /// The tokens a subscription covered, by agent.
    subscription: Vec<TokensRow>,
}

#[derive(Debug, Serialize)]
pub struct TokensRow {
    name: String,
    tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct HarnessRow {
    agent: AgentSlug,
    /// A subscription covered it: tokens only.
    subscription: bool,
    cost_usd: Option<f64>,
    tokens: u64,
    models: Vec<TokensRow>,
}

impl From<PlanWindow> for WindowRow {
    fn from(w: PlanWindow) -> Self {
        Self {
            minutes: w.minutes,
            used_percent: w.used_percent,
            resets_at: w.resets_at.map(epoch_ms),
            scope: w.scope,
        }
    }
}

impl From<PlanLimits> for PlanRow {
    fn from(p: PlanLimits) -> Self {
        Self {
            agent: p.agent,
            plan: p.plan,
            machine_name: p.machine_name,
            as_of: epoch_ms(p.as_of),
            windows: p.windows.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<AgentTokens> for TokensRow {
    fn from(t: AgentTokens) -> Self {
        Self {
            name: t.agent.to_string(),
            tokens: t.tokens,
        }
    }
}

impl From<ProjectSpend> for SpendRow {
    fn from(s: ProjectSpend) -> Self {
        Self {
            project: s.project,
            cost_usd: s.cost_usd,
            unpriced: s.unpriced.into_iter().map(Into::into).collect(),
            metered_tokens: 0,
            subscription: Vec::new(),
        }
    }
}

impl From<ProjectBill> for SpendRow {
    fn from(b: ProjectBill) -> Self {
        Self {
            project: b.project,
            cost_usd: b.cost_usd,
            unpriced: Vec::new(),
            metered_tokens: b.metered_tokens,
            subscription: b.subscription.into_iter().map(Into::into).collect(),
        }
    }
}

fn model_rows(models: Vec<ModelSpend>) -> Vec<TokensRow> {
    models
        .into_iter()
        .map(|m| TokensRow {
            name: m.model.to_string(),
            tokens: m.tokens,
        })
        .collect()
}

impl From<HarnessBill> for HarnessRow {
    fn from(h: HarnessBill) -> Self {
        Self {
            agent: h.agent,
            subscription: h.subscription,
            cost_usd: h.cost_usd,
            tokens: h.tokens,
            models: model_rows(h.models),
        }
    }
}

impl From<SpendPeriod> for PeriodRow {
    fn from(p: SpendPeriod) -> Self {
        Self {
            total: SpendRow {
                project: String::new(),
                cost_usd: p.cost_usd,
                unpriced: Vec::new(),
                metered_tokens: p.metered_tokens,
                subscription: Vec::new(),
            },
            tokens: p.metered_tokens.saturating_add(p.subscription_tokens),
            projects: p.projects.into_iter().map(Into::into).collect(),
            harnesses: p.harnesses.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SpendReport> for UsageRow {
    fn from(r: SpendReport) -> Self {
        Self {
            limits: r.limits.into_iter().map(Into::into).collect(),
            today: r.today.into(),
            week: r.week.into(),
            month: Some(r.month.into()),
            metered: true,
        }
    }
}

impl From<HarnessSpend> for HarnessRow {
    fn from(h: HarnessSpend) -> Self {
        Self {
            agent: h.agent,
            subscription: false,
            cost_usd: h.cost_usd,
            tokens: h.tokens,
            models: model_rows(h.models),
        }
    }
}

impl From<PeriodSpend> for PeriodRow {
    fn from(p: PeriodSpend) -> Self {
        Self {
            total: p.total.into(),
            tokens: p.tokens,
            projects: p.projects.into_iter().map(Into::into).collect(),
            harnesses: p.harnesses.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<UsageReport> for UsageRow {
    fn from(r: UsageReport) -> Self {
        Self {
            limits: r.limits.into_iter().map(Into::into).collect(),
            today: r.today.into(),
            week: r.week.into(),
            month: Some(r.month.into()),
            metered: false,
        }
    }
}

/// An older server's view: today by project and harness, the week as one
/// total, no month.
impl From<UsageView> for UsageRow {
    fn from(v: UsageView) -> Self {
        let mut unpriced: Vec<AgentTokens> = Vec::new();
        for tokens in v.today.iter().flat_map(|s| &s.unpriced) {
            match unpriced.iter_mut().find(|t| t.agent == tokens.agent) {
                Some(t) => t.tokens += tokens.tokens,
                None => unpriced.push(tokens.clone()),
            }
        }
        let today = PeriodSpend {
            total: ProjectSpend {
                project: String::new(),
                cost_usd: v.today.iter().map(|s| s.cost_usd).sum(),
                unpriced,
            },
            tokens: v.harnesses.iter().map(|h| h.tokens).sum(),
            projects: v.today,
            harnesses: v.harnesses,
        };
        let week = PeriodSpend {
            total: v.week,
            tokens: 0,
            projects: Vec::new(),
            harnesses: Vec::new(),
        };
        Self {
            limits: v.limits.into_iter().map(Into::into).collect(),
            today: today.into(),
            week: week.into(),
            month: None,
            metered: false,
        }
    }
}

/// The plans and spending, summed over every machine that is online. Each
/// machine reads its plans again on every call, so `refresh` changes nothing.
#[tauri::command]
pub async fn usage_get(fleet: FleetState<'_>, _refresh: bool) -> Result<Option<UsageRow>> {
    Ok(Some(desk(&fleet).await?.spend().await?.into()))
}
