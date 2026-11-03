//! The pill atop every command center page: how much of each coding
//! agent's plan is used, and what the agents cost today, this week and
//! this month.

use crate::{
    beep::{Beep, epoch_ms},
    error::Result,
    projects::unknown_op,
};
use beep_proto::{
    AgentSlug, AgentTokens, GetUsage, GetUsageReport, HarnessSpend, PeriodSpend, PlanLimits,
    PlanWindow, ProjectSpend, UsageReport, UsageView,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Service<'a> = State<'a, Arc<Beep>>;

/// `UsageReport` as the window reads it, times in milliseconds since the
/// Unix epoch.
#[derive(Debug, Serialize)]
pub struct UsageRow {
    limits: Vec<PlanRow>,
    today: PeriodRow,
    week: PeriodRow,
    /// `None` from a server from before `GetUsageReport`.
    month: Option<PeriodRow>,
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
}

#[derive(Debug, Serialize)]
pub struct TokensRow {
    name: String,
    tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct HarnessRow {
    agent: AgentSlug,
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
        }
    }
}

impl From<HarnessSpend> for HarnessRow {
    fn from(h: HarnessSpend) -> Self {
        Self {
            agent: h.agent,
            cost_usd: h.cost_usd,
            tokens: h.tokens,
            models: h
                .models
                .into_iter()
                .map(|m| TokensRow {
                    name: m.model.to_string(),
                    tokens: m.tokens,
                })
                .collect(),
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
        }
    }
}

/// The plans and spending; `None` from a server too old to keep them, so
/// the pill hides. `refresh` has the machines read the plans again.
#[tauri::command]
pub async fn usage_get(beep: Service<'_>, refresh: bool) -> Result<Option<UsageRow>> {
    match beep.call(GetUsageReport { refresh }).await {
        Ok(report) => return Ok(Some(report.into())),
        Err(e) if unknown_op(&e) => {}
        Err(e) => return Err(e),
    }
    match beep.call(GetUsage { refresh }).await {
        Ok(view) => Ok(Some(view.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}
