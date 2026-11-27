//! What the coding agents cost and how much of their plans is left, summed over every machine.
//! Each online node reports what its agents' own logs say (`agents/1`, `FollowUsage`): the
//! desktop reads that once per call and adds it up. A node says all it has, then each change; it
//! has no end marker, so a read ends when the node has been quiet for [`SETTLE`]. A node reads
//! its plans beside the stream, and Claude Code can take most of half a minute to start: a
//! stream still owing a plan is kept as a [`PlanFollow`] until the node sends it.

use crate::{
    desk::Desk,
    error::Result,
    usage_cache::{PlanFollow, Replies, Said, Session, absorb, plan_limits},
};
use beep_projects::imports::records::{ImportRow, imported_project_of};
use beep_proto::{
    AccountName, AgentSlug, AgentTokens, HarnessBill, ModelId, ModelSpend, PlanLimits, ProjectBill,
    SpendPeriod, SpendReport, UsageTableRow,
    node::{
        agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
        usage::HourSpend,
    },
};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use futures::{StreamExt, future::join_all};
use iroh::EndpointId;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};
use tokio::time::{Instant, timeout};

/// Upper bound on listening to a trickling node in one pass before continuing in the background.
const MAX_LISTEN: Duration = Duration::from_secs(6);
/// Minimum bound on listening to a node in one pass.
const MIN_LISTEN: Duration = Duration::from_secs(2);
/// Minimum connect/open deadline for a usage stream on one node.
const MIN_OPEN: Duration = Duration::from_millis(1200);
/// The agents whose plans a node reads.
const PLAN_AGENTS: [AgentSlug; 2] = [AgentSlug::CLAUDE, AgentSlug::CODEX];

struct Row<'a> {
    /// The imported project the session ran in; none for any other folder.
    project: Option<String>,
    agent: &'a AgentSlug,
    hour: &'a HourSpend,
}

impl Desk {
    /// How long a node is waited on after it was last heard from.
    pub fn settle(mut self, quiet: Duration) -> Self {
        self.settle = quiet;
        self
    }

    #[tracing::instrument(skip_all, fields(peer = %machine.fmt_short()))]
    async fn listen(&self, machine: EndpointId, name: String, said: &mut Said) {
        let open_limit = self.settle.max(MIN_OPEN);
        let Ok(Ok(client)) = timeout(open_limit, self.machines.client(machine)).await else {
            self.hosts
                .usage()
                .merge_and_fill(machine, false, Vec::new(), HashMap::new(), said);
            return;
        };
        let opened = Instant::now();
        let request = SessionsRequest::FollowUsage { history: true };
        let Ok(Ok(replies)) = timeout(
            open_limit,
            client.follow::<_, SessionsReply>(SESSIONS_PROTOCOL, &request),
        )
        .await
        else {
            self.hosts
                .usage()
                .merge_and_fill(machine, false, Vec::new(), HashMap::new(), said);
            return;
        };
        let mut replies: Replies = Box::pin(replies);
        let quiet = self.settle;
        let listen_limit = (self.settle * 4).clamp(MIN_LISTEN, MAX_LISTEN);
        let mut open = true;
        let mut heard: BTreeSet<AgentSlug> = BTreeSet::new();
        let mut machine_plans = Vec::new();
        let mut machine_sessions = HashMap::new();
        let listened = timeout(listen_limit, async {
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
                        machine_plans.push((name.clone(), usage));
                    }
                    SessionsReply::SessionSpend { usage } => {
                        let key = (machine, usage.agent.clone(), usage.session.clone());
                        absorb(
                            machine_sessions.entry(key).or_insert_with(|| Session {
                                cwd: usage.cwd.clone(),
                                hours: BTreeMap::new(),
                                historical: Vec::new(),
                            }),
                            usage,
                        );
                    }
                    _ => {}
                }
            }
        })
        .await;

        let complete = listened.is_ok() && open;
        self.hosts
            .usage()
            .merge_and_fill(machine, complete, machine_plans, machine_sessions, said);

        if !open {
            return;
        }
        // The agents the machine said it has, or both when it has not said.
        let env = self.machines.last_env(machine);
        let runs = |agent: &AgentSlug| {
            env.as_ref()
                .is_none_or(|env| env.tools.iter().any(|tool| tool.name == agent.as_ref()))
        };
        let waiting: BTreeSet<AgentSlug> = PLAN_AGENTS
            .into_iter()
            .filter(|agent| runs(agent) && !heard.contains(agent))
            .collect();
        let incomplete_history = listened.is_err();
        if !waiting.is_empty() || incomplete_history {
            said.follows.push(PlanFollow {
                machine,
                name,
                replies,
                waiting,
                incomplete_history,
                settle: quiet,
                usage: self.hosts.usage().clone(),
                opened,
            });
        }
    }

    async fn said(&self) -> Said {
        let listens = self
            .mesh
            .code_hosts()
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

    pub async fn spend(&self) -> Result<SpendReport> {
        Ok(self.spend_and_follow().await?.0)
    }

    /// [`Desk::spend`], with the streams of the machines that still owe a plan or history tail.
    pub(crate) async fn spend_and_follow(&self) -> Result<(SpendReport, Vec<PlanFollow>)> {
        let mut said = self.said().await;
        let follows = std::mem::take(&mut said.follows);
        Ok((self.report_from_said(&said), follows))
    }

    /// Rebuilds the spend report from cached per-machine state after a slow stream finishes
    /// delivering its remaining `SessionSpend` entries in the background.
    pub(crate) fn spend_from_cache(&self) -> SpendReport {
        let online: BTreeSet<EndpointId> = self
            .mesh
            .code_hosts()
            .into_iter()
            .filter(|status| status.online)
            .map(|status| status.device.id)
            .collect();
        let said = self.hosts.usage().snapshot(&online);
        self.report_from_said(&said)
    }

    fn report_from_said(&self, said: &Said) -> SpendReport {
        let imports = crate::projects::MeshStore::new(&self.mesh).rows();
        let (today, week, month) = periods();
        let rows = |from: i64| rows_since(said, &imports, from);
        SpendReport {
            limits: freshest(
                said.plans
                    .iter()
                    .filter_map(|(machine, plan)| plan_limits(machine, plan)),
            ),
            today: bill(&rows(today)),
            week: bill(&rows(week)),
            month: bill(&rows(month)),
            historical: historical_rows(said, &imports),
        }
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

/// The hours of every session from `from` on, each with the imported project it ran in.
fn rows_since<'a>(said: &'a Said, imports: &[ImportRow], from: i64) -> Vec<Row<'a>> {
    let from = u64::try_from(from).unwrap_or(0);
    said.sessions
        .iter()
        .flat_map(|((machine, agent, _), session)| {
            let project = imported_project_of(machine, &session.cwd, imports);
            session
                .hours
                .values()
                .filter(move |hour| hour.hour >= from)
                .map(move |hour| Row {
                    project: project.clone(),
                    agent,
                    hour,
                })
        })
        .collect()
}

fn local_date(at: u64) -> Option<NaiveDate> {
    DateTime::from_timestamp(i64::try_from(at).ok()?, 0)
        .map(|time| time.with_timezone(&Local).date_naive())
}

fn historical_rows(said: &Said, imports: &[ImportRow]) -> Vec<UsageTableRow> {
    let mut rows = Vec::new();
    for ((machine, agent, _), session) in &said.sessions {
        let project = imported_project_of(machine, &session.cwd, imports);
        for usage in &session.historical {
            rows.push(UsageTableRow {
                first: usage
                    .first
                    .and_then(local_date)
                    .map(|date| date.to_string()),
                last: usage.last.and_then(local_date).map(|date| date.to_string()),
                project: project.clone(),
                agent: Some(agent.clone()),
                cost_usd: if usage.billing.on_subscription(agent) {
                    0.0
                } else {
                    usage.cost_usd
                },
                tokens: usage.tokens,
            });
        }
    }
    rows.sort_by(|a, b| {
        b.last
            .cmp(&a.last)
            .then(b.first.cmp(&a.first))
            .then(a.project.cmp(&b.project))
            .then_with(|| cmp_cost(a.cost_usd, b.cost_usd))
            .then(b.tokens.cmp(&a.tokens))
    });
    rows
}

fn daily_rows(rows: &[Row<'_>]) -> Vec<UsageTableRow> {
    let mut days = BTreeMap::new();
    for row in rows {
        let Some(day) = local_date(row.hour.hour) else {
            continue;
        };
        let entry = days
            .entry((std::cmp::Reverse(day), &row.project))
            .or_insert((0.0, 0_u64));
        if metered(row) {
            entry.0 += row.hour.cost_usd;
        }
        entry.1 = entry.1.saturating_add(row.hour.tokens());
    }
    days.into_iter()
        .map(
            |((std::cmp::Reverse(day), project), (cost_usd, tokens))| UsageTableRow {
                first: Some(day.to_string()),
                last: Some(day.to_string()),
                project: project.clone(),
                agent: None,
                cost_usd,
                tokens,
            },
        )
        .collect()
}

/// The freshest report of each agent's account: Claude Code, then Codex, then the others, each
/// default account before its named ones.
pub(crate) fn freshest(plans: impl IntoIterator<Item = PlanLimits>) -> Vec<PlanLimits> {
    let mut by_agent: BTreeMap<(AgentSlug, Option<AccountName>), PlanLimits> = BTreeMap::new();

    for plan in plans {
        let key = (plan.agent.clone(), plan.account.clone());

        if by_agent
            .get(&key)
            .is_none_or(|held| plan.as_of >= held.as_of)
        {
            by_agent.insert(key, plan);
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
            .then_with(|| a.account.cmp(&b.account))
    });
    plans
}

/// `plan` as `machine` read it; `None` without windows or balances.
fn cmp_cost(a: f64, b: f64) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
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

fn by_project<'r>(rows: &'r [Row<'_>]) -> BTreeMap<&'r str, Vec<&'r Row<'r>>> {
    let mut by: BTreeMap<&str, Vec<&Row<'_>>> = BTreeMap::new();
    // Only imported projects are listed; the rest count in the totals.
    for row in rows {
        if let Some(project) = &row.project {
            by.entry(project.as_str()).or_default().push(row);
        }
    }

    by
}

fn by_agent<'r>(rows: &'r [Row<'_>]) -> BTreeMap<&'r AgentSlug, Vec<&'r Row<'r>>> {
    let mut by: BTreeMap<&AgentSlug, Vec<&Row<'_>>> = BTreeMap::new();

    for row in rows {
        by.entry(row.agent).or_default().push(row);
    }

    by
}

fn metered(row: &Row<'_>) -> bool {
    !row.hour.billing.on_subscription(row.agent)
}

fn bill(rows: &[Row<'_>]) -> SpendPeriod {
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
    let all: Vec<&Row<'_>> = rows.iter().collect();
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
            // A subscription bill carries its use at API prices as an estimate,
            // shown apart: it is in no total.
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
        daily: daily_rows(rows),
    }
}

#[cfg(test)]
#[path = "tests/usage.rs"]
mod tests;
