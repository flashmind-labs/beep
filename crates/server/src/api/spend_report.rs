//! What the caller's coding agents cost, metered spend apart from subscription use.

use super::{Reply, usage::limits};
use crate::{
    api::router::Cx,
    protocol::{
        AgentSlug, AgentTokens, GetSpendReport, HarnessBill, ModelId, ModelSpend, ProjectBill,
        SpendPeriod, SpendReport, node::usage::Billing,
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

pub(super) async fn get_spend_report(
    cx: Cx,
    GetSpendReport { refresh }: GetSpendReport,
) -> Reply<SpendReport> {
    if refresh {
        for node in cx.app.nodes.of_user(cx.user.id()) {
            node.refresh_usage();
        }
    }
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

/// Whether a subscription covered `agent`'s use; unknown is one for Claude Code and Codex.
fn on_subscription(agent: &AgentSlug, billing: Billing) -> bool {
    match billing {
        Billing::Subscription => true,
        Billing::Metered => false,
        Billing::Unknown => *agent == AgentSlug::CLAUDE || *agent == AgentSlug::CODEX,
    }
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
        let subscription = on_subscription(&row.agent, row.billing);
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
            let key = (&row.agent, on_subscription(&row.agent, row.billing));
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
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn machine() -> EndpointId {
        SecretKey::from_bytes(&[1; 32]).public()
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).expect("a real day")
    }

    fn spent(
        cwd: &str,
        agent: &str,
        billing: Billing,
        cost: Option<f64>,
        tokens: u64,
    ) -> DaySpendRow {
        DaySpendRow {
            endpoint: machine(),
            cwd: cwd.into(),
            agent: agent.into(),
            day: day(3),
            billing,
            cost_usd: cost,
            tokens,
            unpriced_tokens: if cost.is_some() { 0 } else { tokens },
        }
    }

    #[test]
    fn dollars_count_only_metered_use() {
        let days = [
            spent(
                "/srv/widget",
                "claude",
                Billing::Unknown,
                Some(361.58),
                1_000_000,
            ),
            spent("/srv/widget", "claude", Billing::Metered, Some(2.0), 10_000),
            spent(
                "/srv/widget",
                "codex",
                Billing::Subscription,
                Some(13.22),
                50_000,
            ),
            spent("/srv/notes", "flsh", Billing::Unknown, Some(13.01), 20_000),
            spent("/srv/notes", "pi", Billing::Metered, None, 5_000),
        ];
        let models = [
            DayModelRow {
                agent: "claude".into(),
                model: "claude-opus-5-5".into(),
                day: day(3),
                billing: Billing::Unknown,
                tokens: 1_000_000,
                from_logs: true,
            },
            DayModelRow {
                agent: "claude".into(),
                model: "us.anthropic.claude-opus-5-5".into(),
                day: day(3),
                billing: Billing::Metered,
                tokens: 10_000,
                from_logs: true,
            },
        ];
        let period = spend_period(&days, &models, &[], day(1));
        assert!((period.cost_usd - 15.01).abs() < 1e-9);
        assert_eq!(period.metered_tokens, 35_000);
        assert_eq!(period.subscription_tokens, 1_050_000);

        let shown: Vec<(&str, bool, Option<f64>, u64)> = period
            .harnesses
            .iter()
            .map(|h| (h.agent.as_str(), h.subscription, h.cost_usd, h.tokens))
            .collect();
        assert_eq!(
            shown,
            [
                ("flsh", false, Some(13.01), 20_000),
                ("claude", false, Some(2.0), 10_000),
                ("claude", true, None, 1_000_000),
                ("codex", true, None, 50_000),
                ("pi", false, None, 5_000),
            ]
        );
        assert_eq!(
            period.harnesses[1].models[0].model,
            "us.anthropic.claude-opus-5-5"
        );
        assert_eq!(period.harnesses[2].models[0].model, "claude-opus-5-5");

        let notes = &period.projects[0];
        assert_eq!(notes.project, "notes");
        assert!((notes.cost_usd - 13.01).abs() < 1e-9);
        assert_eq!(notes.metered_tokens, 25_000);
        let widget = &period.projects[1];
        assert!((widget.cost_usd - 2.0).abs() < 1e-9);
        let covered: Vec<(&str, u64)> = widget
            .subscription
            .iter()
            .map(|t| (t.agent.as_str(), t.tokens))
            .collect();
        assert_eq!(covered, [("claude", 1_000_000), ("codex", 50_000)]);

        assert!(
            spend_period(&days, &models, &[], day(4))
                .projects
                .is_empty()
        );
    }
}
