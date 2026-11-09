//! What the caller's coding agents cost, metered spend apart from subscription use.

use super::{Reply, usage::limits};
use crate::{
    api::router::Cx,
    protocol::{
        AgentSlug, AgentTokens, GetSpendReport, HarnessBill, ModelId, ModelSpend, ProjectBill,
        SpendPeriod, SpendReport,
    },
    store::{DayModelRow, DaySpendRow},
};
use beep_projects::imports::records::{ImportRow, project_of};
use chrono::NaiveDate;
use iroh::EndpointId;
use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeMap, HashMap, HashSet},
};

pub(super) async fn get_spend_report(cx: Cx, _: GetSpendReport) -> Reply<SpendReport> {
    let periods = cx.user.usage_periods().await?;
    let (days, models, plans, imports, nodes) = tokio::try_join!(
        cx.user.day_spend(&periods),
        cx.user.day_models(&periods),
        cx.user.plan_usages(),
        cx.user.imports(),
        cx.user.nodes(),
    )?;
    let names: HashMap<EndpointId, String> = nodes
        .into_iter()
        .filter_map(|node| Some((node.endpoint.parse().ok()?, node.name)))
        .collect();
    let period = |from: NaiveDate| spend_period(&days, &models, &imports, from);
    Ok(SpendReport {
        limits: limits(plans, &names),
        today: period(periods.today),
        week: period(periods.week),
        month: period(periods.month),
    })
}

/// One harness paid for one way.
type HarnessKey<'a> = (&'a AgentSlug, bool);

/// What was spent from the day `from` on, by project and by harness.
fn spend_period(
    days: &[DaySpendRow],
    models: &[DayModelRow],
    imports: &[ImportRow],
    from: NaiveDate,
) -> SpendPeriod {
    let mut cost_usd = 0.0;
    let mut metered_tokens = 0;
    let mut subscription_tokens = 0;
    let mut projects: BTreeMap<String, ProjectBill> = BTreeMap::new();
    let mut harnesses: BTreeMap<HarnessKey, HarnessBill> = BTreeMap::new();

    for row in days.iter().filter(|row| row.day >= from) {
        let subscription = row.billing.on_subscription(&row.agent);
        let project = project_of(&row.endpoint, &row.cwd, imports);
        let bill = projects
            .entry(project.clone())
            .or_insert_with(|| ProjectBill {
                project,
                cost_usd: 0.0,
                metered_tokens: 0,
                subscription: Vec::new(),
            });
        let harness = harnesses
            .entry((&row.agent, subscription))
            .or_insert_with(|| HarnessBill {
                agent: row.agent.clone(),
                subscription,
                cost_usd: None,
                tokens: 0,
                models: Vec::new(),
            });
        harness.tokens += row.tokens;
        if subscription {
            subscription_tokens += row.tokens;

            match bill.subscription.iter_mut().find(|t| t.agent == row.agent) {
                Some(tokens) => tokens.tokens += row.tokens,
                None => bill.subscription.push(AgentTokens {
                    agent: row.agent.clone(),
                    tokens: row.tokens,
                }),
            }
        } else {
            let cost = row.cost_usd.unwrap_or(0.0);
            cost_usd += cost;
            metered_tokens += row.tokens;
            bill.cost_usd += cost;
            bill.metered_tokens += row.tokens;

            if let Some(cost) = row.cost_usd {
                *harness.cost_usd.get_or_insert(0.0) += cost;
            }
        }
    }

    for ((agent, subscription), tokens) in period_models(models, from) {
        if let Some(harness) = harnesses.get_mut(&(agent, subscription)) {
            harness.models = tokens;
        }
    }

    let mut projects: Vec<ProjectBill> = projects
        .into_values()
        .filter(|p| p.cost_usd > 0.0 || p.metered_tokens > 0 || !p.subscription.is_empty())
        .collect();
    for project in &mut projects {
        project.subscription.sort_by_key(|t| Reverse(t.tokens));
    }

    let project_tokens =
        |p: &ProjectBill| p.metered_tokens + p.subscription.iter().map(|t| t.tokens).sum::<u64>();
    projects.sort_by(|a, b| {
        by_cost(a.cost_usd, b.cost_usd).then_with(|| project_tokens(b).cmp(&project_tokens(a)))
    });
    let mut harnesses: Vec<HarnessBill> = harnesses
        .into_values()
        .filter(|h| h.tokens > 0 || h.cost_usd.is_some_and(|cost| cost > 0.0))
        .collect();
    harnesses.sort_by(|a, b| {
        by_cost(a.cost_usd.unwrap_or(0.0), b.cost_usd.unwrap_or(0.0)).then(b.tokens.cmp(&a.tokens))
    });
    SpendPeriod {
        cost_usd,
        metered_tokens,
        subscription_tokens,
        projects,
        harnesses,
    }
}

/// The most expensive first.
fn by_cost(a: f64, b: f64) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

/// Tokens by harness, billing and model since `from`, most first; logs, else watched turns.
fn period_models(
    models: &[DayModelRow],
    from: NaiveDate,
) -> BTreeMap<HarnessKey<'_>, Vec<ModelSpend>> {
    let models: Vec<&DayModelRow> = models.iter().filter(|row| row.day >= from).collect();
    let logged: HashSet<&AgentSlug> = models
        .iter()
        .filter(|row| row.from_logs)
        .map(|row| &row.agent)
        .collect();
    let mut by_model: BTreeMap<(HarnessKey, &ModelId), u64> = BTreeMap::new();

    for row in models {
        if row.from_logs == logged.contains(&row.agent) {
            let key = (&row.agent, row.billing.on_subscription(&row.agent));
            *by_model.entry((key, &row.model)).or_default() += row.tokens;
        }
    }

    let mut out: BTreeMap<HarnessKey, Vec<ModelSpend>> = BTreeMap::new();

    for ((key, model), tokens) in by_model {
        out.entry(key).or_default().push(ModelSpend {
            model: model.clone(),
            tokens,
        });
    }

    for list in out.values_mut() {
        list.sort_by_key(|model| Reverse(model.tokens));
    }

    out
}

#[cfg(test)]
#[path = "../tests/api_spend_report.rs"]
mod tests;
