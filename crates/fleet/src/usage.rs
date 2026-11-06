//! What the coding agents cost and how much of their plans is left, summed over every machine.
//! Each online node reports what its agents' own logs say (`agents/1`, `FollowUsage`): the
//! desktop reads that once per call and adds it up. A node says all it has, then each change; it
//! has no end marker, so a read ends when the node has been quiet for [`SETTLE`]. A node reads
//! its plans beside the stream, and Claude Code can take most of half a minute to start: a
//! stream still owing a plan is kept as a [`PlanFollow`] until the node sends it.

use crate::{desk::Desk, error::Result};
use beep_mesh::MeshError;
use beep_projects::imports::records::{ImportRow, project_of};
use beep_proto::{
    AgentSlug, AgentTokens, HarnessBill, HarnessSpend, ModelId, ModelSpend, PeriodSpend,
    PlanLimits, PlanWindow, ProjectBill, ProjectSpend, SpendPeriod, SpendReport, UsageReport,
    UsageView,
    node::{
        agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
        usage::{HourSpend, PlanUsage, SessionSpend},
    },
};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use futures::{Stream, StreamExt, future::join_all};
use iroh::EndpointId;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap},
    pin::Pin,
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::{Instant, timeout};

/// How long a node may be quiet before what it has said is taken as all of it.
pub const SETTLE: Duration = Duration::from_secs(2);
/// The longest a node is listened to.
const LISTEN: Duration = Duration::from_secs(20);
/// How long a stream is kept for the plans it owes, from its opening: a node gives each CLI 30
/// seconds to answer, its start included.
const PLAN_WAIT: Duration = Duration::from_secs(35);
/// The agents whose plans a node reads.
const PLAN_AGENTS: [AgentSlug; 2] = [AgentSlug::CLAUDE, AgentSlug::CODEX];

type Replies = Pin<Box<dyn Stream<Item = std::result::Result<SessionsReply, MeshError>> + Send>>;

/// A machine's usage stream kept after a read, for the plans it has not sent yet.
pub(crate) struct PlanFollow {
    name: String,
    replies: Replies,
    /// The agents the machine runs whose plan it has not sent.
    pub(crate) waiting: BTreeSet<AgentSlug>,
    opened: Instant,
}

impl PlanFollow {
    /// Hands each plan the node sends to `heard`, until it sent every agent waited on, the
    /// stream ends or [`PLAN_WAIT`] has passed since it opened.
    pub(crate) async fn run(mut self, heard: impl Fn(PlanLimits) + Send) {
        let left = PLAN_WAIT.saturating_sub(self.opened.elapsed());
        let listened = timeout(left, async {
            while !self.waiting.is_empty() {
                let Some(Ok(reply)) = self.replies.next().await else {
                    return;
                };
                if let SessionsReply::PlanUsage { usage } = reply {
                    self.waiting.remove(&usage.agent);
                    if let Some(limits) = plan_limits(&self.name, &usage) {
                        heard(limits);
                    }
                }
            }
        })
        .await;
        if listened.is_err() {
            log::debug!("{} sent no plan for {:?}", self.name, self.waiting);
        }
    }
}

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
    follows: Vec<PlanFollow>,
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
        let opened = Instant::now();
        let request = SessionsRequest::FollowUsage { history: true };
        let Ok(replies) = client
            .follow::<_, SessionsReply>(SESSIONS_PROTOCOL, &request)
            .await
        else {
            return;
        };
        let mut replies: Replies = Box::pin(replies);
        let quiet = self.settle;
        let mut open = true;
        let mut heard: BTreeSet<AgentSlug> = BTreeSet::new();
        let listened = timeout(LISTEN, async {
            loop {
                let reply = match timeout(quiet, replies.next()).await {
                    Ok(Some(Ok(reply))) => reply,
                    Ok(_) => {
                        open = false;
                        return;
                    }
                    // Quiet: what it had is said.
                    Err(_) => return,
                };
                match reply {
                    SessionsReply::PlanUsage { usage } => {
                        heard.insert(usage.agent.clone());
                        said.plans.push((name.clone(), usage));
                    }
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
        if listened.is_err() || !open {
            return;
        }
        // The agents the machine said it has, or both when it has not said.
        let env = self.peers.last_env(machine);
        let runs = |agent: &AgentSlug| {
            env.as_ref()
                .is_none_or(|env| env.tools.iter().any(|tool| tool.name == agent.as_str()))
        };
        let waiting: BTreeSet<AgentSlug> = PLAN_AGENTS
            .into_iter()
            .filter(|agent| runs(agent) && !heard.contains(agent))
            .collect();
        if !waiting.is_empty() {
            said.follows.push(PlanFollow {
                name,
                replies,
                waiting,
                opened,
            });
        }
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
            all.follows.extend(said.follows);
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
        Ok(self.spend_and_follow().await?.0)
    }

    /// [`Desk::spend`], with the streams of the machines that still owe a plan.
    pub(crate) async fn spend_and_follow(&self) -> Result<(SpendReport, Vec<PlanFollow>)> {
        let mut said = self.said().await;
        let follows = std::mem::take(&mut said.follows);
        let imports = self.imports();
        let (today, week, month) = periods();
        let rows = |from: i64| Rows::of(&said, &imports, from);
        let report = SpendReport {
            limits: limits(&said),
            today: bill(&rows(today)),
            week: bill(&rows(week)),
            month: bill(&rows(month)),
        };
        Ok((report, follows))
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
    freshest(
        said.plans
            .iter()
            .filter_map(|(machine, plan)| plan_limits(machine, plan)),
    )
}

/// The freshest report of each agent: Claude Code, then Codex, then the others.
pub(crate) fn freshest(plans: impl IntoIterator<Item = PlanLimits>) -> Vec<PlanLimits> {
    let mut by_agent: BTreeMap<AgentSlug, PlanLimits> = BTreeMap::new();
    for plan in plans {
        if by_agent
            .get(&plan.agent)
            .is_none_or(|held| plan.as_of >= held.as_of)
        {
            by_agent.insert(plan.agent.clone(), plan);
        }
    }
    let rank = |agent: &AgentSlug| match agent {
        a if *a == AgentSlug::CLAUDE => 0,
        a if *a == AgentSlug::CODEX => 1,
        _ => 2,
    };
    let mut plans: Vec<PlanLimits> = by_agent.into_values().collect();
    plans.sort_by(|a, b| {
        rank(&a.agent)
            .cmp(&rank(&b.agent))
            .then_with(|| a.agent.cmp(&b.agent))
    });
    plans
}

/// `plan` as `machine` read it; `None` without windows.
fn plan_limits(machine: &str, plan: &PlanUsage) -> Option<PlanLimits> {
    if plan.windows.is_empty() {
        return None;
    }
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
    Some(PlanLimits {
        agent: plan.agent.clone(),
        plan: plan.plan.clone(),
        machine_name: machine.to_owned(),
        as_of: UNIX_EPOCH + Duration::from_secs(plan.at),
        windows,
    })
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
    !row.hour.billing.on_subscription(row.agent)
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
            // A subscription bill carries its use at API prices as an estimate. The
            // desktop reads this report in process and shows it apart: it is in no
            // total, and the server's replies keep `None` there.
            let cost: f64 = part.iter().map(|row| row.hour.cost_usd).sum();
            harnesses.push(HarnessBill {
                agent: agent.clone(),
                subscription,
                cost_usd: (cost > 0.0).then_some(cost),
                tokens: spent,
                models: models(part.into_iter()),
            });
        }
    }
    let spent = |h: &HarnessBill| {
        if h.subscription {
            0.0
        } else {
            h.cost_usd.unwrap_or(0.0)
        }
    };
    harnesses.sort_by(|a, b| cmp_cost(spent(a), spent(b)).then(b.tokens.cmp(&a.tokens)));
    SpendPeriod {
        cost_usd: paid(&all),
        metered_tokens: tokens(&all, false),
        subscription_tokens: tokens(&all, true),
        projects,
        harnesses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::usage::Billing;

    fn hour(billing: Billing) -> HourSpend {
        HourSpend {
            hour: 0,
            model: ModelId::new("some-model"),
            input_tokens: 1_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_usd: 5.0,
            unpriced_tokens: 0,
            billing,
        }
    }

    #[test]
    fn a_plan_that_could_not_be_told_reports_tokens_not_dollars() {
        let unknown = hour(Billing::Unknown);
        let paid = hour(Billing::Metered);
        let rows = Rows(vec![
            Row {
                project: "widget".to_owned(),
                agent: &AgentSlug::CLAUDE,
                hour: &unknown,
            },
            Row {
                project: "widget".to_owned(),
                agent: &AgentSlug::CLAUDE,
                hour: &paid,
            },
        ]);

        let bill = bill(&rows);

        assert_eq!(bill.cost_usd, 5.0);
        assert_eq!(bill.metered_tokens, 1_000);
        assert_eq!(bill.subscription_tokens, 1_000);
        let covered = bill.harnesses.iter().find(|h| h.subscription);
        assert_eq!(covered.and_then(|h| h.cost_usd), Some(5.0));
        let metered = bill.harnesses.iter().find(|h| !h.subscription);
        assert_eq!(metered.and_then(|h| h.cost_usd), Some(5.0));
    }
}
