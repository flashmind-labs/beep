//! What the nodes said of spend and plans (`Said`), kept across reads (`UsageCache`).

use beep_mesh::MeshError;
use beep_proto::SessionId;
use beep_proto::{
    AgentSlug, ModelId, PlanLimits, PlanWindow,
    node::{
        agent::SessionsReply,
        usage::{HistoricalSpend, HourSpend, PlanUsage, SessionSpend},
    },
};
use futures::{Stream, StreamExt};
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::{Instant, timeout};

/// How long a node may be quiet before what it has said is taken as all of it.
/// How long a stream is kept for the plans it owes, from its opening: a node gives each CLI 30
/// seconds to answer, its start included.
pub(crate) const PLAN_WAIT: Duration = Duration::from_secs(35);
pub(crate) const SETTLE: Duration = Duration::from_secs(2);
pub(crate) type Replies =
    Pin<Box<dyn Stream<Item = std::result::Result<SessionsReply, MeshError>> + Send>>;

/// A machine's usage stream kept after a read, for the plans or remaining history it owes.
pub(crate) struct PlanFollow {
    pub(crate) machine: EndpointId,
    pub(crate) name: String,
    pub(crate) replies: Replies,
    /// The agents the machine runs whose plan it has not sent.
    pub(crate) waiting: BTreeSet<AgentSlug>,
    /// True when the initial read hit `MAX_LISTEN` before the history scan settled.
    pub(crate) incomplete_history: bool,
    pub(crate) settle: Duration,
    pub(crate) usage: Arc<UsageCache>,
    pub(crate) opened: Instant,
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
pub(crate) struct Session {
    pub(crate) cwd: String,
    pub(crate) hours: BTreeMap<(u64, ModelId), HourSpend>,
    pub(crate) historical: Vec<HistoricalSpend>,
}

/// Last known usage reported by one machine, retained across transient link drops and partial reads.
#[derive(Clone, Default)]
pub(crate) struct MachineSaid {
    pub(crate) plans: Vec<(String, PlanUsage)>,
    pub(crate) sessions: HashMap<(EndpointId, AgentSlug, SessionId), Session>,
}

pub(crate) fn merge_plans(into: &mut Vec<(String, PlanUsage)>, incoming: Vec<(String, PlanUsage)>) {
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
    pub(crate) known: Mutex<HashMap<EndpointId, MachineSaid>>,
}

impl UsageCache {
    pub(crate) fn absorb_plan(&self, machine: EndpointId, name: String, plan: PlanUsage) {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = known.entry(machine).or_default();
        merge_plans(&mut entry.plans, vec![(name, plan)]);
    }

    pub(crate) fn absorb_session(&self, machine: EndpointId, usage: SessionSpend) {
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
    pub(crate) fn merge_and_fill(
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

    pub(crate) fn snapshot(&self, online: &BTreeSet<EndpointId>) -> Said {
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
pub(crate) struct Said {
    pub(crate) plans: Vec<(String, PlanUsage)>,
    pub(crate) sessions: HashMap<(EndpointId, AgentSlug, SessionId), Session>,
    pub(crate) follows: Vec<PlanFollow>,
}

/// A report replaces the session's hours from `since` on.
pub(crate) fn absorb(session: &mut Session, usage: SessionSpend) {
    session.historical = usage.historical;
    session.hours.retain(|(hour, _), _| *hour < usage.since);

    for hour in usage.hours {
        session.hours.insert((hour.hour, hour.model.clone()), hour);
    }
}

pub(crate) fn plan_limits(machine: &str, plan: &PlanUsage) -> Option<PlanLimits> {
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
