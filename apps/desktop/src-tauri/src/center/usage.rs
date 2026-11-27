//! The pill atop every command center page: how much of each coding
//! agent's plan is used, and what the agents cost today, this week and
//! this month.

use crate::{beep::epoch_ms, error::Result, fleet::projects::FleetState};
use beep_proto::{
    AccountName, AgentSlug, CreditUnit, HarnessBill, ModelSpend, PlanLimits, PlanWindow,
    ProjectBill, ProviderCredits, SpendCell, SpendPeriod, SpendReport, UsageTableRow,
};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;
use std::str::FromStr;

/// `SpendReport` as the window reads it, times in milliseconds since the
/// Unix epoch.
#[derive(Debug, Serialize)]
pub struct UsageRow {
    limits: Vec<PlanRow>,
    today: PeriodRow,
    week: PeriodRow,
    month: PeriodRow,
    historical: Vec<UsageTableRow>,
}

#[derive(Debug, Serialize)]
pub struct PeriodRow {
    total: SpendRow,
    tokens: u64,
    projects: Vec<SpendRow>,
    harnesses: Vec<HarnessRow>,
    daily: Vec<UsageTableRow>,
    cells: Vec<SpendCell>,
}

#[derive(Debug, Serialize)]
pub struct PlanRow {
    agent: AgentSlug,
    /// The agent's named account; `None` for the default one.
    account: Option<AccountName>,
    plan: Option<String>,
    machine_name: String,
    as_of: u64,
    windows: Vec<WindowRow>,
    credits: Vec<CreditRow>,
}

/// A provider's balance with its amounts written out, each `None` when
/// unknown or not a number.
#[derive(Debug, Serialize)]
pub struct CreditRow {
    #[serde(flatten)]
    credit: ProviderCredits,
    /// What is left in whole dollars for the bar, such as `$622`.
    remaining_short: Option<String>,
    /// What is left to the cent for the popover, such as `$622.32`.
    remaining_full: Option<String>,
    used_full: Option<String>,
    total_full: Option<String>,
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
    cost_usd: f64,
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
            credits: p.credits.into_iter().map(Into::into).collect(),
            agent: p.agent,
            account: p.account,
            plan: p.plan,
            machine_name: p.machine_name,
            as_of: epoch_ms(p.as_of),
            windows: p.windows.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ProviderCredits> for CreditRow {
    fn from(credit: ProviderCredits) -> Self {
        let amount = |value: &Option<String>| {
            value
                .as_deref()
                .and_then(|text| Amount::parse(credit.unit, text))
        };
        let remaining = amount(&credit.remaining);
        let used = amount(&credit.used);
        let total = amount(&credit.total);
        Self {
            remaining_short: remaining.map(|a| a.label(0)),
            remaining_full: remaining.map(|a| a.label(2)),
            used_full: used.map(|a| a.label(2)),
            total_full: total.map(|a| a.label(2)),
            credit,
        }
    }
}

/// A balance as a provider reports it: an exact decimal in its unit.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Amount {
    unit: CreditUnit,
    value: Decimal,
}

impl Amount {
    /// `text` as a decimal, such as `622.3218671` or `1e3`.
    fn parse(unit: CreditUnit, text: &str) -> Option<Self> {
        let text = text.trim();
        let value = Decimal::from_str(text)
            .or_else(|_| Decimal::from_scientific(text))
            .ok()?;
        Some(Self { unit, value })
    }

    /// The amount rounded half up (away from zero) to `places` decimals,
    /// with a `$` for dollars: `$622` or `$622.32`.
    fn label(self, places: u32) -> String {
        let mut value = self
            .value
            .round_dp_with_strategy(places, RoundingStrategy::MidpointAwayFromZero);

        if value.is_zero() {
            value = Decimal::ZERO;
        }

        value.rescale(places);
        let sign = if value.is_sign_negative() { "-" } else { "" };
        let digits = value.abs();

        match self.unit {
            CreditUnit::Usd => format!("{sign}${digits}"),
            CreditUnit::Credits | CreditUnit::Unknown => format!("{sign}{digits}"),
        }
    }
}

impl From<ProjectBill> for SpendRow {
    fn from(b: ProjectBill) -> Self {
        Self {
            cost_usd: b.cost_usd,
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
                cost_usd: p.cost_usd,
            },
            daily: p.daily,
            cells: p.cells,
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
            month: r.month.into(),
            historical: r.historical,
        }
    }
}

/// The plans and spending, summed over every machine that is online. Each
/// machine reads its plans again on every read. The last report comes at
/// once, with a new one collected behind it; `refresh` waits for a new one.
#[tauri::command]
pub async fn usage_get(fleet: FleetState<'_>, refresh: bool) -> Result<Option<UsageRow>> {
    Ok(Some(fleet.spend(refresh).await?.into()))
}

#[cfg(test)]
#[path = "../tests/center_usage.rs"]
mod tests;
