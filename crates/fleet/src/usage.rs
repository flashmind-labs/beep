//! What the coding agents cost and how much of their plans is left, summed over every machine.
//! Each online node reports what its agents' own logs say (`agents/1`, `FollowUsage`): the
//! desktop reads that once per call and adds it up. A node says all it has, then each change; it
//! has no end marker, so a read ends when the node has been quiet for [`SETTLE`].

use crate::{desk::Desk, error::Result};
use beep_projects::imports::records::{ImportRow, project_of};
use beep_proto::{
    AgentSlug, AgentTokens, HarnessBill, HarnessSpend, ModelId, ModelSpend, PeriodSpend,
    PlanLimits, PlanWindow, ProjectBill, ProjectSpend, SpendPeriod, SpendReport, UsageReport,
    UsageView,
    node::{
        agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
        usage::{Billing, HourSpend, PlanUsage, SessionSpend},
    },
};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use futures::{StreamExt, future::join_all};
use iroh::EndpointId;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::timeout;

/// How long a node may be quiet before what it has said is taken as all of it.
pub const SETTLE: Duration = Duration::from_secs(2);
/// The longest a node is listened to.
const LISTEN: Duration = Duration::from_secs(20);

/// One session's spend on one machine, by hour.
struct Session {
    machine: EndpointId,
    cwd: String,
    agent: AgentSlug,
    hours: BTreeMap<(u64, ModelId), HourSpend>,
}

/// What the nodes said.
#[derive(Default)]
struct Said {
    plans: Vec<(String, PlanUsage)>,
    sessions: HashMap<(EndpointId, AgentSlug, String), Session>,
}

/// What one period spent.
struct Rows<'a>(Vec<Row<'a>>);

struct Row<'a> {
    project: String,
    agent: &'a AgentSlug,
    hour: &'a HourSpend,
}

impl Desk {
    /// How long a node is waited on after it was last heard from.
    pub fn settle(mut self, quiet: Duration) -> Self {
        self.settle = quiet;
        self
    }

    async fn listen(&self, machine: EndpointId, name: String, said: &mut Said) {
        let Ok(client) = self.peers.client(machine).await else {
            return;
        };
        let request = SessionsRequest::FollowUsage { history: true };
        let Ok(replies) = client
            .follow::<_, SessionsReply>(SESSIONS_PROTOCOL, &request)
            .await
        else {
            return;
        };
        let mut replies = Box::pin(replies);
        let quiet = self.settle;
        let _ = timeout(LISTEN, async {
            while let Ok(Some(Ok(reply))) = timeout(quiet, replies.next()).await {
                match reply {
                    SessionsReply::PlanUsage { usage } => said.plans.push((name.clone(), usage)),
                    SessionsReply::SessionSpend { usage } => {
                        let key = (machine, usage.agent.clone(), usage.session.clone());
                        absorb(
                            said.sessions.entry(key).or_insert_with(|| Session {
                                machine,
                                cwd: usage.cwd.clone(),
                                agent: usage.agent.clone(),
                                hours: BTreeMap::new(),
                            }),
                            usage,
                        );
                    }
                    _ => {}
                }
            }
        })
        .await;
    }

    async fn said(&self) -> Said {
        let listens = self
            .mesh
            .devices()
            .into_iter()
            .filter(|status| status.online)
            .map(|status| async move {
                let mut said = Said::default();
                self.listen(status.device.id, status.device.name, &mut said)
                    .await;
                said
            });
        let mut all = Said::default();
        for said in join_all(listens).await {
            all.plans.extend(said.plans);
            all.sessions.extend(said.sessions);
        }
        all
    }

    fn imports(&self) -> Vec<ImportRow> {
        crate::projects::MeshStore::new(&self.mesh).rows()
    }

    pub async fn usage(&self) -> Result<UsageView> {
        let report = self.usage_report().await?;
        Ok(UsageView {
            limits: report.limits,
            today: report.today.projects,
            harnesses: report.today.harnesses,
            week: ProjectSpend {
                project: String::new(),
                ..report.week.total
            },
        })
    }

    pub async fn usage_report(&self) -> Result<UsageReport> {
        let said = self.said().await;
        let imports = self.imports();
        let (today, week, month) = periods();
        let rows = |from: i64| Rows::of(&said, &imports, from);
        Ok(UsageReport {
            limits: limits(&said),
            today: period(&rows(today)),
            week: period(&rows(week)),
            month: period(&rows(month)),
        })
    }

    pub async fn spend(&self) -> Result<SpendReport> {
        let said = self.said().await;
        let imports = self.imports();
        let (today, week, month) = periods();
        let rows = |from: i64| Rows::of(&said, &imports, from);
        Ok(SpendReport {
            limits: limits(&said),
            today: bill(&rows(today)),
            week: bill(&rows(week)),
            month: bill(&rows(month)),
        })
    }
}

/// A report replaces the session's hours from `since` on.
fn absorb(session: &mut Session, usage: SessionSpend) {
    session.hours.retain(|(hour, _), _| *hour < usage.since);
    for hour in usage.hours {
        session.hours.insert((hour.hour, hour.model.clone()), hour);
    }
}

/// The starts of today, this week (Monday) and this month, in Unix seconds, in local time.
fn periods() -> (i64, i64, i64) {
    let today = Local::now().date_naive();
    let monday = today - chrono::Days::new(u64::from(today.weekday().num_days_from_monday()));
    let first = today.with_day(1).unwrap_or(today);
    (start(today), start(monday), start(first))
}

fn start(day: NaiveDate) -> i64 {
    let midnight = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    Local.from_local_datetime(&midnight).earliest().map_or_else(
        || midnight.and_utc().timestamp(),
        |at: DateTime<Local>| at.timestamp(),
    )
}

impl<'a> Rows<'a> {
    fn of(said: &'a Said, imports: &[ImportRow], from: i64) -> Self {
        let from = u64::try_from(from).unwrap_or(0);
        Self(
            said.sessions
                .values()
                .flat_map(|session| {
                    let project = project_of(&session.machine, &session.cwd, imports);
                    session
                        .hours
                        .values()
                        .filter(move |hour| hour.hour >= from)
                        .map(move |hour| Row {
                            project: project.clone(),
                            agent: &session.agent,
                            hour,
                        })
                })
                .collect(),
        )
    }
}

fn limits(said: &Said) -> Vec<PlanLimits> {
    // The freshest report of each agent.
    let mut freshest: BTreeMap<&AgentSlug, &(String, PlanUsage)> = BTreeMap::new();
    for plan in &said.plans {
        let held = freshest.entry(&plan.1.agent).or_insert(plan);
        if plan.1.at >= held.1.at {
            *held = plan;
        }
    }
    let rank = |agent: &AgentSlug| match agent {
        a if *a == AgentSlug::CLAUDE => 0,
        a if *a == AgentSlug::CODEX => 1,
        _ => 2,
    };
    let mut plans: Vec<&(String, PlanUsage)> = freshest.into_values().collect();
    plans.sort_by(|a, b| {
        rank(&a.1.agent)
            .cmp(&rank(&b.1.agent))
            .then_with(|| a.1.agent.cmp(&b.1.agent))
    });
    plans
        .into_iter()
        .filter(|(_, plan)| !plan.windows.is_empty())
        .map(|(machine, plan)| {
            let mut windows: Vec<PlanWindow> = plan
                .windows
                .iter()
                .map(|window| PlanWindow {
                    minutes: window.minutes,
                    used_percent: window.used_percent,
                    resets_at: window
                        .resets_at
                        .map(|at| UNIX_EPOCH + Duration::from_secs(at)),
                    scope: window.scope.clone(),
                })
                .collect();
            windows.sort_by_key(|window| window.minutes);
            PlanLimits {
                agent: plan.agent.clone(),
                plan: plan.plan.clone(),
                machine_name: machine.clone(),
                as_of: UNIX_EPOCH + Duration::from_secs(plan.at),
                windows,
            }
        })
        .collect()
}

fn cmp_cost(a: f64, b: f64) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

fn total<'r>(rows: impl Iterator<Item = &'r Row<'r>>, project: &str) -> ProjectSpend {
    let mut cost_usd = 0.0;
    let mut unpriced: BTreeMap<&AgentSlug, u64> = BTreeMap::new();
    for row in rows {
        cost_usd += row.hour.cost_usd;
        if row.hour.unpriced_tokens > 0 {
            *unpriced.entry(row.agent).or_default() += row.hour.unpriced_tokens;
        }
    }
    ProjectSpend {
        project: project.to_owned(),
        cost_usd,
        unpriced: unpriced
            .into_iter()
            .map(|(agent, tokens)| AgentTokens {
                agent: agent.clone(),
                tokens,
            })
            .collect(),
    }
}

/// Tokens by model of `agent`'s rows, the most first.
fn models<'r>(rows: impl Iterator<Item = &'r Row<'r>>) -> Vec<ModelSpend> {
    let mut by_model: BTreeMap<&ModelId, u64> = BTreeMap::new();
    for row in rows {
        *by_model.entry(&row.hour.model).or_default() += row.hour.tokens();
    }
    let mut out: Vec<ModelSpend> = by_model
        .into_iter()
        .map(|(model, tokens)| ModelSpend {
            model: model.clone(),
            tokens,
        })
        .collect();
    out.sort_by_key(|model| std::cmp::Reverse(model.tokens));
    out
}

fn by_project<'r>(rows: &'r Rows<'_>) -> BTreeMap<&'r str, Vec<&'r Row<'r>>> {
    let mut by: BTreeMap<&str, Vec<&Row<'_>>> = BTreeMap::new();
    for row in &rows.0 {
        by.entry(row.project.as_str()).or_default().push(row);
    }
    by
}

fn by_agent<'r>(rows: &'r Rows<'_>) -> BTreeMap<&'r AgentSlug, Vec<&'r Row<'r>>> {
    let mut by: BTreeMap<&AgentSlug, Vec<&Row<'_>>> = BTreeMap::new();
    for row in &rows.0 {
        by.entry(row.agent).or_default().push(row);
    }
    by
}

fn period(rows: &Rows<'_>) -> PeriodSpend {
    let mut projects: Vec<ProjectSpend> = by_project(rows)
        .into_iter()
        .map(|(name, rows)| total(rows.into_iter(), name))
        .filter(|spend| spend.cost_usd > 0.0 || !spend.unpriced.is_empty())
        .collect();
    projects.sort_by(|a, b| cmp_cost(a.cost_usd, b.cost_usd));
    let mut harnesses: Vec<HarnessSpend> = by_agent(rows)
        .into_iter()
        .map(|(agent, rows)| {
            let cost: f64 = rows.iter().map(|row| row.hour.cost_usd).sum();
            HarnessSpend {
                agent: agent.clone(),
                cost_usd: (cost > 0.0).then_some(cost),
                tokens: rows.iter().map(|row| row.hour.tokens()).sum(),
                models: models(rows.into_iter()),
            }
        })
        .filter(|harness| harness.tokens > 0)
        .collect();
    harnesses.sort_by(|a, b| {
        cmp_cost(a.cost_usd.unwrap_or(0.0), b.cost_usd.unwrap_or(0.0)).then(b.tokens.cmp(&a.tokens))
    });
    PeriodSpend {
        total: total(rows.0.iter(), ""),
        tokens: rows.0.iter().map(|row| row.hour.tokens()).sum(),
        projects,
        harnesses,
    }
}

fn metered(row: &Row<'_>) -> bool {
    row.hour.billing != Billing::Subscription
}

fn bill(rows: &Rows<'_>) -> SpendPeriod {
    let paid = |rows: &[&Row<'_>]| -> f64 {
        rows.iter()
            .filter(|row| metered(row))
            .map(|row| row.hour.cost_usd)
            .sum()
    };
    let tokens = |rows: &[&Row<'_>], subscription: bool| -> u64 {
        rows.iter()
            .filter(|row| metered(row) != subscription)
            .map(|row| row.hour.tokens())
            .sum()
    };
    let all: Vec<&Row<'_>> = rows.0.iter().collect();
    let mut projects: Vec<ProjectBill> = by_project(rows)
        .into_iter()
        .map(|(name, rows)| {
            let mut subscription: BTreeMap<&AgentSlug, u64> = BTreeMap::new();
            for row in rows.iter().filter(|row| !metered(row)) {
                *subscription.entry(row.agent).or_default() += row.hour.tokens();
            }
            ProjectBill {
                project: name.to_owned(),
                cost_usd: paid(&rows),
                metered_tokens: tokens(&rows, false),
                subscription: subscription
                    .into_iter()
                    .map(|(agent, tokens)| AgentTokens {
                        agent: agent.clone(),
                        tokens,
                    })
                    .collect(),
            }
        })
        .filter(|bill| {
            bill.cost_usd > 0.0 || bill.metered_tokens > 0 || !bill.subscription.is_empty()
        })
        .collect();
    projects.sort_by(|a, b| {
        cmp_cost(a.cost_usd, b.cost_usd).then(b.metered_tokens.cmp(&a.metered_tokens))
    });
    let mut harnesses = Vec::new();
    for (agent, rows) in by_agent(rows) {
        for subscription in [false, true] {
            let part: Vec<&Row<'_>> = rows
                .iter()
                .copied()
                .filter(|row| metered(row) != subscription)
                .collect();
            let spent: u64 = part.iter().map(|row| row.hour.tokens()).sum();
            if spent == 0 {
                continue;
            }
            let cost = paid(&part);
            harnesses.push(HarnessBill {
                agent: agent.clone(),
                subscription,
                cost_usd: (!subscription && cost > 0.0).then_some(cost),
                tokens: spent,
                models: models(part.into_iter()),
            });
        }
    }
    harnesses.sort_by(|a, b| {
        cmp_cost(a.cost_usd.unwrap_or(0.0), b.cost_usd.unwrap_or(0.0)).then(b.tokens.cmp(&a.tokens))
    });
    SpendPeriod {
        cost_usd: paid(&all),
        metered_tokens: tokens(&all, false),
        subscription_tokens: tokens(&all, true),
        projects,
        harnesses,
    }
}
