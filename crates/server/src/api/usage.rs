//! What the caller's coding agents cost and how much of their plans is
//! left, for the pill atop the command center.

use super::Reply;
use crate::{
    protocol::{
        AgentTokens, GetUsage, HarnessSpend, ModelSpend, PlanLimits, PlanWindow, ProjectSpend,
        UsageView,
    },
    router::Cx,
    store::{ImportRow, ModelRow, PlanRow, SpendRow},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    time::{Duration, UNIX_EPOCH},
};

/// The harnesses whose plans show first, in this order.
const PLAN_ORDER: &[&str] = &["claude", "codex"];

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
    let names: HashMap<String, String> = nodes
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect();
    Ok(UsageView {
        limits: limits(plans, &names),
        today: today(&spend, &imports),
        harnesses: harnesses(&spend, &models),
        week: total(spend.iter()),
    })
}

/// Each agent's plan, Claude Code and Codex first.
fn limits(mut plans: Vec<PlanRow>, names: &HashMap<String, String>) -> Vec<PlanLimits> {
    let rank = |agent: &str| {
        PLAN_ORDER
            .iter()
            .position(|known| *known == agent)
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
    let mut tokens: BTreeMap<&str, u64> = BTreeMap::new();
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
                agent: agent.to_owned(),
                tokens,
            })
            .collect(),
    }
}

/// Today's spending by harness, only those that spent anything, the most
/// expensive first: dollars where the harness reports them, tokens always.
fn harnesses(spend: &[SpendRow], models: &[ModelRow]) -> Vec<HarnessSpend> {
    let mut by_agent: BTreeMap<&str, HarnessSpend> = BTreeMap::new();
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
        if let Some(harness) = by_agent.get_mut(model.agent.as_str()) {
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

/// The project a session in `cwd` on the machine `endpoint` belongs to:
/// the imported project whose folder there holds it, the innermost when
/// they nest, else the folder's own name.
fn project_of(endpoint: &str, cwd: &str, imports: &[ImportRow]) -> String {
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
    use crate::protocol::node::usage::LimitWindow;
    use chrono::{DateTime, Utc};
    use std::time::SystemTime;

    fn row(cwd: &str, agent: &str, today: bool, cost: Option<f64>, tokens: u64) -> SpendRow {
        SpendRow {
            endpoint: "box".into(),
            cwd: cwd.into(),
            agent: agent.into(),
            today,
            cost_usd: cost,
            tokens,
            unpriced_tokens: if cost.is_some() { 0 } else { tokens },
        }
    }

    fn import(folder: &str, project: &str) -> ImportRow {
        ImportRow {
            project: project.into(),
            endpoint: "box".into(),
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
            project_of("box", "/srv/work/widget", &imports),
            "acme/widget"
        );
        assert_eq!(
            project_of("box", "/srv/work/widget/.worktrees/fix", &imports),
            "acme/widget"
        );
        assert_eq!(
            project_of("box", "/srv/work/widget/vendor/relay/src", &imports),
            "acme/relay"
        );
        assert_eq!(project_of("box", "/srv/work/widgets", &imports), "widgets");
        assert_eq!(project_of("other", "/srv/work/widget", &imports), "widget");
    }

    #[test]
    fn today_and_the_week_add_up_by_project_and_harness() {
        let imports = [import("/srv/widget", "acme/widget")];
        let spend = [
            row("/srv/widget", "claude", true, Some(3.1), 5_000),
            row("/srv/widget", "codex", true, None, 1_100_000),
            row("/srv/relay", "opencode", true, Some(1.1), 900),
            row("/srv/relay", "claude", false, Some(27.6), 9_000),
            row("/srv/quiet", "gemini", true, None, 0),
        ];
        let today = today(&spend, &imports);
        let shown: Vec<(&str, f64)> = today
            .iter()
            .map(|p| (p.project.as_str(), p.cost_usd))
            .collect();
        assert_eq!(shown, [("acme/widget", 3.1), ("relay", 1.1)]);
        assert_eq!(
            today[0].unpriced,
            [AgentTokens {
                agent: "codex".into(),
                tokens: 1_100_000
            }]
        );
        let week = total(spend.iter());
        assert!((week.cost_usd - 31.8).abs() < 1e-9);

        let models = [ModelRow {
            agent: "codex".into(),
            model: "gpt-5-codex".into(),
            tokens: 1_100_000,
        }];
        let harnesses = harnesses(&spend, &models);
        let shown: Vec<(&str, Option<f64>, u64)> = harnesses
            .iter()
            .map(|h| (h.agent.as_str(), h.cost_usd, h.tokens))
            .collect();
        assert_eq!(
            shown,
            [
                ("claude", Some(3.1), 5_000),
                ("opencode", Some(1.1), 900),
                ("codex", None, 1_100_000)
            ]
        );
        assert_eq!(harnesses[2].models[0].model, "gpt-5-codex");
    }

    #[test]
    fn plans_show_claude_then_codex_with_their_machine() {
        let plan = |agent: &str| PlanRow {
            agent: agent.into(),
            endpoint: "box".into(),
            plan: Some("pro".into()),
            windows: vec![LimitWindow {
                minutes: 300,
                used_percent: 38.0,
                resets_at: Some(100),
                scope: None,
            }],
            read_at: SystemTime::now().into(),
        };
        let names = HashMap::from([("box".to_owned(), "build box".to_owned())]);
        let empty = PlanRow {
            windows: Vec::new(),
            ..plan("pi")
        };
        let shown = limits(
            vec![plan("opencode"), plan("codex"), plan("claude"), empty],
            &names,
        );
        let agents: Vec<&str> = shown.iter().map(|p| p.agent.as_str()).collect();
        assert_eq!(agents, ["claude", "codex", "opencode"]);
        assert_eq!(shown[0].machine_name, "build box");
        assert_eq!(
            shown[0].windows[0].resets_at,
            Some(UNIX_EPOCH + Duration::from_secs(100))
        );
    }
}
