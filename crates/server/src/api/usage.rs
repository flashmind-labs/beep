//! What the caller's coding agents cost and how much of their plans is left.

use super::Reply;
use crate::{
    protocol::{
        AgentSlug, AgentTokens, GetUsage, GetUsageReport, HarnessSpend, ModelId, ModelSpend,
        PeriodSpend, PlanLimits, PlanWindow, ProjectSpend, UsageReport, UsageView,
    },
    router::Cx,
    store::{DayModelRow, DaySpendRow, ImportRow, ModelRow, PlanRow, SpendRow},
};
use chrono::NaiveDate;
use iroh::EndpointId;
use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeMap, HashMap, HashSet},
    time::{Duration, UNIX_EPOCH},
};

/// The harnesses whose plans show first, in this order.
const PLAN_ORDER: &[AgentSlug] = &[AgentSlug::CLAUDE, AgentSlug::CODEX];

pub(super) async fn get_usage(cx: Cx, GetUsage { refresh }: GetUsage) -> Reply<UsageView> {
    if refresh {
        for node in cx.app.nodes.of_user(cx.user.id()) {
            node.refresh_usage();
        }
    }
    let (spend, models, plans, imports, nodes) = tokio::try_join!(
        cx.user.week_spend(),
        cx.user.today_models(),
        cx.user.plan_usages(),
        cx.user.imports(),
        cx.user.nodes(),
    )?;
    // A node whose key does not parse shows its plan without a machine.
    let names: HashMap<EndpointId, String> = nodes
        .into_iter()
        .filter_map(|node| Some((node.endpoint.parse().ok()?, node.name)))
        .collect();
    Ok(UsageView {
        limits: limits(plans, &names),
        today: today(&spend, &imports),
        harnesses: harnesses(&spend, &models),
        week: total(spend.iter()),
    })
}

pub(super) async fn get_usage_report(
    cx: Cx,
    GetUsageReport { refresh }: GetUsageReport,
) -> Reply<UsageReport> {
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
    let period = |from: NaiveDate| period_spend(&days, &models, &imports, from);
    Ok(UsageReport {
        limits: limits(plans, &names),
        today: period(periods.today),
        week: period(periods.week),
        month: period(periods.month),
    })
}

/// What was spent from the day `from` on, by project and by harness.
fn period_spend(
    days: &[DaySpendRow],
    models: &[DayModelRow],
    imports: &[ImportRow],
    from: NaiveDate,
) -> PeriodSpend {
    // As this week's rows, every one counted as today's.
    let rows: Vec<SpendRow> = days
        .iter()
        .filter(|row| row.day >= from)
        .map(|row| SpendRow {
            endpoint: row.endpoint,
            cwd: row.cwd.clone(),
            agent: row.agent.clone(),
            today: true,
            cost_usd: row.cost_usd,
            tokens: row.tokens,
            unpriced_tokens: row.unpriced_tokens,
        })
        .collect();
    PeriodSpend {
        total: total(rows.iter()),
        tokens: rows.iter().map(|row| row.tokens).sum(),
        projects: today(&rows, imports),
        harnesses: harnesses(&rows, &period_models(models, from)),
    }
}

/// Tokens by agent and model since `from`, most first: from logs, else turns Beep watched.
fn period_models(models: &[DayModelRow], from: NaiveDate) -> Vec<ModelRow> {
    let models: Vec<&DayModelRow> = models.iter().filter(|row| row.day >= from).collect();
    let logged: HashSet<&AgentSlug> = models
        .iter()
        .filter(|row| row.from_logs)
        .map(|row| &row.agent)
        .collect();
    let mut by_model: BTreeMap<(&AgentSlug, &ModelId), u64> = BTreeMap::new();
    for row in models {
        if row.from_logs == logged.contains(&row.agent) {
            *by_model.entry((&row.agent, &row.model)).or_default() += row.tokens;
        }
    }
    let mut out: Vec<ModelRow> = by_model
        .into_iter()
        .map(|((agent, model), tokens)| ModelRow {
            agent: agent.clone(),
            model: model.clone(),
            tokens,
        })
        .collect();
    out.sort_by_key(|row| Reverse(row.tokens));
    out
}

/// Each agent's plan, Claude Code and Codex first.
pub(super) fn limits(
    mut plans: Vec<PlanRow>,
    names: &HashMap<EndpointId, String>,
) -> Vec<PlanLimits> {
    let rank = |agent: &AgentSlug| {
        PLAN_ORDER
            .iter()
            .position(|known| known == agent)
            .unwrap_or(PLAN_ORDER.len())
    };
    plans.sort_by(|a, b| {
        rank(&a.agent)
            .cmp(&rank(&b.agent))
            .then(a.agent.cmp(&b.agent))
    });
    plans
        .into_iter()
        .filter(|plan| !plan.windows.is_empty())
        .map(|plan| PlanLimits {
            machine_name: names.get(&plan.endpoint).cloned().unwrap_or_default(),
            agent: plan.agent,
            plan: plan.plan,
            as_of: plan.read_at.into(),
            windows: plan
                .windows
                .into_iter()
                .map(|window| PlanWindow {
                    minutes: window.minutes,
                    used_percent: window.used_percent,
                    resets_at: window
                        .resets_at
                        .map(|at| UNIX_EPOCH + Duration::from_secs(at)),
                    scope: window.scope,
                })
                .collect(),
        })
        .collect()
}

/// Today's spending by project, the most expensive first.
fn today(spend: &[SpendRow], imports: &[ImportRow]) -> Vec<ProjectSpend> {
    let mut by_project: BTreeMap<String, Vec<&SpendRow>> = BTreeMap::new();
    for row in spend.iter().filter(|row| row.today) {
        by_project
            .entry(project_of(&row.endpoint, &row.cwd, imports))
            .or_default()
            .push(row);
    }
    let mut today: Vec<ProjectSpend> = by_project
        .into_iter()
        .map(|(project, rows)| ProjectSpend {
            project,
            ..total(rows.into_iter())
        })
        .filter(|spend| spend.cost_usd > 0.0 || !spend.unpriced.is_empty())
        .collect();
    today.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(Ordering::Equal)
            .then_with(|| unpriced(b).cmp(&unpriced(a)))
    });
    today
}

fn unpriced(spend: &ProjectSpend) -> u64 {
    spend.unpriced.iter().map(|agent| agent.tokens).sum()
}

/// What `rows` spent together, under no project.
fn total<'a>(rows: impl Iterator<Item = &'a SpendRow>) -> ProjectSpend {
    let mut cost_usd = 0.0;
    let mut tokens: BTreeMap<&AgentSlug, u64> = BTreeMap::new();
    for row in rows {
        cost_usd += row.cost_usd.unwrap_or(0.0);
        if row.unpriced_tokens > 0 {
            *tokens.entry(&row.agent).or_default() += row.unpriced_tokens;
        }
    }
    ProjectSpend {
        project: String::new(),
        cost_usd,
        unpriced: tokens
            .into_iter()
            .map(|(agent, tokens)| AgentTokens {
                agent: agent.clone(),
                tokens,
            })
            .collect(),
    }
}

/// Today's spending by harness that spent anything, most expensive first.
fn harnesses(spend: &[SpendRow], models: &[ModelRow]) -> Vec<HarnessSpend> {
    let mut by_agent: BTreeMap<&AgentSlug, HarnessSpend> = BTreeMap::new();
    for row in spend.iter().filter(|row| row.today) {
        let harness = by_agent.entry(&row.agent).or_insert_with(|| HarnessSpend {
            agent: row.agent.clone(),
            cost_usd: None,
            tokens: 0,
            models: Vec::new(),
        });
        if let Some(cost) = row.cost_usd {
            *harness.cost_usd.get_or_insert(0.0) += cost;
        }
        harness.tokens += row.tokens;
    }
    // Rows come the most used first.
    for model in models {
        if let Some(harness) = by_agent.get_mut(&model.agent) {
            harness.models.push(ModelSpend {
                model: model.model.clone(),
                tokens: model.tokens,
            });
        }
    }
    let mut harnesses: Vec<HarnessSpend> = by_agent
        .into_values()
        .filter(|h| h.tokens > 0 || h.cost_usd.is_some_and(|cost| cost > 0.0))
        .collect();
    harnesses.sort_by(|a, b| {
        let cost = |h: &HarnessSpend| h.cost_usd.unwrap_or(0.0);
        cost(b)
            .partial_cmp(&cost(a))
            .unwrap_or(Ordering::Equal)
            .then(b.tokens.cmp(&a.tokens))
    });
    harnesses
}

/// The innermost imported project holding `cwd` on `endpoint`, else the folder's own name.
pub(super) fn project_of(endpoint: &EndpointId, cwd: &str, imports: &[ImportRow]) -> String {
    let endpoint = endpoint.to_string();
    let separator = |c: char| c == '/' || c == '\\';
    let cwd = cwd.trim_end_matches(separator);
    let holds = |folder: &str| {
        let folder = folder.trim_end_matches(separator);
        !folder.is_empty()
            && cwd
                .strip_prefix(folder)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(separator))
    };
    imports
        .iter()
        .filter(|import| import.endpoint == endpoint && holds(&import.folder))
        .max_by_key(|import| import.folder.len())
        .map(|import| import.project.clone())
        .unwrap_or_else(|| match cwd.rsplit(separator).next() {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => cwd.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use iroh::SecretKey;

    fn machine(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn import(folder: &str, project: &str) -> ImportRow {
        ImportRow {
            project: project.into(),
            endpoint: machine(1).to_string(),
            folder: folder.into(),
            url: String::new(),
            imported_at: DateTime::<Utc>::UNIX_EPOCH,
            fork: None,
        }
    }

    #[test]
    fn sessions_go_to_the_imported_project_that_holds_them() {
        let imports = [
            import("/srv/work/widget", "acme/widget"),
            import("/srv/work/widget/vendor/relay", "acme/relay"),
        ];
        assert_eq!(
            project_of(&machine(1), "/srv/work/widget", &imports),
            "acme/widget"
        );
        assert_eq!(
            project_of(&machine(1), "/srv/work/widget/.worktrees/fix", &imports),
            "acme/widget"
        );
        assert_eq!(
            project_of(&machine(1), "/srv/work/widget/vendor/relay/src", &imports),
            "acme/relay"
        );
        assert_eq!(
            project_of(&machine(1), "/srv/work/widgets", &imports),
            "widgets"
        );
        assert_eq!(
            project_of(&machine(2), "/srv/work/widget", &imports),
            "widget"
        );
    }
}
