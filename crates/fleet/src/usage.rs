//! What the coding agents cost and how much of their plans is left, summed over every machine.
//! Each online node reports what its agents' own logs say (`agents/1`, `FollowUsage`): the
//! desktop reads that once per call and adds it up. A node says all it has, then each change; it
//! has no end marker, so a read ends when the node has been quiet for [`SETTLE`]. A node reads
//! its plans beside the stream, and Claude Code can take most of half a minute to start: a
//! stream still owing a plan is kept as a [`PlanFollow`] until the node sends it.

use crate::{desk::Desk, error::Result};
use beep_mesh::MeshError;
use beep_projects::imports::records::{ImportRow, imported_project_of};
use beep_proto::SessionId;
use beep_proto::{
    AccountName, AgentSlug, AgentTokens, HarnessBill, ModelId, ModelSpend, PlanLimits, PlanWindow,
    ProjectBill, SpendPeriod, SpendReport, UsageTableRow,
    node::{
        agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
        usage::{HistoricalSpend, HourSpend, PlanUsage, SessionSpend},
    },
};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use futures::{Stream, StreamExt, future::join_all};
use iroh::EndpointId;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::{Instant, timeout};

/// How long a node may be quiet before what it has said is taken as all of it.
pub const SETTLE: Duration = Duration::from_secs(2);
/// Upper bound on listening to a trickling node in one pass before continuing in the background.
const MAX_LISTEN: Duration = Duration::from_secs(6);
/// Minimum bound on listening to a node in one pass.
const MIN_LISTEN: Duration = Duration::from_secs(2);
/// Minimum connect/open deadline for a usage stream on one node.
const MIN_OPEN: Duration = Duration::from_millis(1200);
/// How long a stream is kept for the plans it owes, from its opening: a node gives each CLI 30
/// seconds to answer, its start included.
const PLAN_WAIT: Duration = Duration::from_secs(35);
/// The agents whose plans a node reads.
const PLAN_AGENTS: [AgentSlug; 2] = [AgentSlug::CLAUDE, AgentSlug::CODEX];

type Replies = Pin<Box<dyn Stream<Item = std::result::Result<SessionsReply, MeshError>> + Send>>;

/// A machine's usage stream kept after a read, for the plans or remaining history it owes.
pub(crate) struct PlanFollow {
    machine: EndpointId,
    name: String,
    replies: Replies,
    /// The agents the machine runs whose plan it has not sent.
    waiting: BTreeSet<AgentSlug>,
    /// True when the initial read hit `MAX_LISTEN` before the history scan settled.
    incomplete_history: bool,
    settle: Duration,
    usage: Arc<UsageCache>,
    opened: Instant,
}

impl PlanFollow {
    /// Hands each plan or completed history tail the node sends to the caller, until every
    /// owed plan and history scan finished, the stream ends or [`PLAN_WAIT`] has passed.
    pub(crate) async fn run(
        mut self,
        heard: impl Fn(PlanLimits) + Send,
        history_updated: impl Fn() + Send,
    ) {
        let left = PLAN_WAIT.saturating_sub(self.opened.elapsed());
        let mut pending_spend = false;
        let listened = timeout(left, async {
            while !self.waiting.is_empty() || self.incomplete_history {
                let next = if self.incomplete_history {
                    match timeout(self.settle, self.replies.next()).await {
                        Ok(item) => item,
                        Err(_) => {
                            self.incomplete_history = false;
                            if pending_spend {
                                pending_spend = false;
                                history_updated();
                            }
                            continue;
                        }
                    }
                } else {
                    self.replies.next().await
                };

                let Some(Ok(reply)) = next else {
                    if pending_spend {
                        history_updated();
                    }
                    return;
                };

                match reply {
                    SessionsReply::PlanUsage { usage } => {
                        self.waiting.remove(&usage.agent);
                        self.usage
                            .absorb_plan(self.machine, self.name.clone(), usage.clone());

                        if let Some(limits) = plan_limits(&self.name, &usage) {
                            heard(limits);
                        }
                    }
                    SessionsReply::SessionSpend { usage } => {
                        self.usage.absorb_session(self.machine, usage);
                        pending_spend = true;
                    }
                    _ => {}
                }
            }
        })
        .await;

        if pending_spend {
            history_updated();
        }

        if listened.is_err() && !self.waiting.is_empty() {
            log::debug!("{} sent no plan for {:?}", self.name, self.waiting);
        }
    }
}

/// One session's spend on one machine, by hour.
#[derive(Clone)]
struct Session {
    cwd: String,
    hours: BTreeMap<(u64, ModelId), HourSpend>,
    historical: Vec<HistoricalSpend>,
}

/// Last known usage reported by one machine, retained across transient link drops and partial reads.
#[derive(Clone, Default)]
struct MachineSaid {
    plans: Vec<(String, PlanUsage)>,
    sessions: HashMap<(EndpointId, AgentSlug, SessionId), Session>,
}

fn merge_plans(into: &mut Vec<(String, PlanUsage)>, incoming: Vec<(String, PlanUsage)>) {
    for (name, plan) in incoming {
        if let Some(existing) = into
            .iter_mut()
            .find(|(_, p)| p.agent == plan.agent && p.account == plan.account)
        {
            if plan.at >= existing.1.at {
                *existing = (name, plan);
            }
        } else {
            into.push((name, plan));
        }
    }
}

/// Per-machine spend cache so a slow or flaky node neither stalls other machines nor drops to 0.
#[derive(Default)]
pub(crate) struct UsageCache {
    known: Mutex<HashMap<EndpointId, MachineSaid>>,
}

impl UsageCache {
    fn absorb_plan(&self, machine: EndpointId, name: String, plan: PlanUsage) {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = known.entry(machine).or_default();
        merge_plans(&mut entry.plans, vec![(name, plan)]);
    }

    fn absorb_session(&self, machine: EndpointId, usage: SessionSpend) {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = known.entry(machine).or_default();
        let key = (machine, usage.agent.clone(), usage.session.clone());
        absorb(
            entry.sessions.entry(key).or_insert_with(|| Session {
                cwd: usage.cwd.clone(),
                hours: BTreeMap::new(),
                historical: Vec::new(),
            }),
            usage,
        );
    }

    /// Merges a machine's pass into the cache and appends the resulting full machine state to `said`.
    /// Only a `complete` pass (the stream went quiet normally) replaces old session entries; a
    /// partial or timed-out pass preserves cached sessions not yet resent.
    fn merge_and_fill(
        &self,
        machine: EndpointId,
        complete: bool,
        plans: Vec<(String, PlanUsage)>,
        sessions: HashMap<(EndpointId, AgentSlug, SessionId), Session>,
        said: &mut Said,
    ) {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = known.entry(machine).or_default();
        merge_plans(&mut entry.plans, plans);

        if complete {
            entry.sessions = sessions;
        } else {
            for (key, incoming) in sessions {
                match entry.sessions.get_mut(&key) {
                    Some(existing) => {
                        existing.cwd = incoming.cwd;
                        existing.historical = incoming.historical;
                        for (hk, hour) in incoming.hours {
                            existing.hours.insert(hk, hour);
                        }
                    }
                    None => {
                        entry.sessions.insert(key, incoming);
                    }
                }
            }
        }

        said.plans.extend(entry.plans.iter().cloned());
        said.sessions
            .extend(entry.sessions.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    fn snapshot(&self, online: &BTreeSet<EndpointId>) -> Said {
        let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let mut said = Said::default();

        for (machine, entry) in known.iter() {
            if online.contains(machine) {
                said.plans.extend(entry.plans.iter().cloned());
                said.sessions
                    .extend(entry.sessions.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }

        said
    }
}

/// What the nodes said.
#[derive(Default)]
struct Said {
    plans: Vec<(String, PlanUsage)>,
    sessions: HashMap<(EndpointId, AgentSlug, SessionId), Session>,
    follows: Vec<PlanFollow>,
}

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
        let Ok(Ok(client)) = timeout(open_limit, self.peers.client(machine)).await else {
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
        let env = self.peers.last_env(machine);
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

/// A report replaces the session's hours from `since` on.
fn absorb(session: &mut Session, usage: SessionSpend) {
    session.historical = usage.historical;
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
fn plan_limits(machine: &str, plan: &PlanUsage) -> Option<PlanLimits> {
    if plan.windows.is_empty() && plan.credits.is_empty() {
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
        credits: plan.credits.clone(),
        account: plan.account.clone(),
    })
}

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
