//! Starting and steering coding-agent sessions from the app: the machines
//! that can run one, starting one (on a named machine or by Auto), reading
//! one in full and sending it a message, a stop or a close.

use super::{
    Reply,
    agent_auto::{self, Candidate},
    agent_sessions::{
        Host, IN_FLIGHT, NODE_LIMIT, beep_session, external_session, limited, linked_keys,
    },
    machines::machine_capability,
};
use crate::{
    instance::Privilege,
    nodes::{Follower, Online, agent_name},
    protocol::{
        node::agent::{self, PlanStatus, Request, SessionInfo},
        *,
    },
    router::Cx,
    tools::machines::command_name,
};
use futures::{StreamExt, stream};
use tokio::time::timeout;

/// The most transcript entries one view reads.
const MAX_ENTRIES: u32 = 200;
/// The agent Auto starts when the request names none, in this order of
/// preference among those the machine offers.
const PREFERRED: [&str; 5] = ["claude", "codex", "gemini", "pi", "flsh"];

/// An online machine of the caller's that offers a coding agent.
struct Live {
    host: AgentHost,
    online: Online,
}

impl Live {
    fn host(&self) -> Host {
        Host {
            id: self.host.machine_id.clone(),
            name: self.host.machine_name.clone(),
        }
    }

    fn offers(&self, agent: &str) -> bool {
        self.host.agents.iter().any(|offered| offered == agent)
    }

    /// `named`, or the one this machine prefers when the call names none.
    fn agent(&self, named: Option<&str>) -> Result<String, ApiError> {
        if let Some(named) = named {
            return if self.offers(named) {
                Ok(named.to_owned())
            } else {
                Err(ApiError::bad_request(format!(
                    "{} does not offer {}.",
                    self.host.machine_name,
                    agent_name(named)
                )))
            };
        }
        PREFERRED
            .iter()
            .find(|agent| self.offers(agent))
            .map(|agent| (*agent).to_owned())
            .or_else(|| self.host.agents.first().cloned())
            .ok_or_else(|| {
                ApiError::bad_request(format!(
                    "{} offers no coding agent.",
                    self.host.machine_name
                ))
            })
    }
}

/// The caller's online machines that offer an agent, in the order the
/// store lists them. A machine whose key was unlinked is left out.
async fn live_hosts(cx: &Cx) -> Result<Vec<Live>, ApiError> {
    if !cx.app.allows(Privilege::Nodes, &cx.user) {
        return Err(ApiError::Unauthorized {});
    }
    let keys = linked_keys(cx).await?;
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .filter(|row| keys.contains(&row.endpoint))
        .filter_map(|row| {
            let online = cx
                .app
                .nodes
                .online(&row.endpoint)
                .filter(|online| online.user == cx.user.id())?;
            let agents: Vec<String> = online
                .offers
                .iter()
                .filter_map(|offer| agent::agent_of(offer))
                .map(str::to_owned)
                .collect();
            if agents.is_empty() {
                return None;
            }
            let env = row.env.unwrap_or_default();
            let host = AgentHost {
                machine_id: row.endpoint,
                machine_name: row.name,
                agents,
                workspace: online.workspace.clone(),
                home: Some(env.home.clone()).filter(|home| !home.is_empty()),
                capabilities: env
                    .capabilities()
                    .into_iter()
                    .map(machine_capability)
                    .collect(),
            };
            Some(Live { host, online })
        })
        .collect())
}

/// The caller's online machine `machine_id`, when it offers an agent.
async fn live_host(cx: &Cx, machine_id: &str) -> Result<Live, ApiError> {
    live_hosts(cx)
        .await?
        .into_iter()
        .find(|live| live.host.machine_id == machine_id)
        .ok_or(ApiError::NotFound {})
}

/// Admins only, like `ListMachines`.
pub(super) async fn list_agent_hosts(cx: Cx, _: ListAgentHosts) -> Reply<Vec<AgentHost>> {
    Ok(live_hosts(&cx)
        .await?
        .into_iter()
        .map(|live| live.host)
        .collect())
}

/// Admins only. Auto reads what the prompt and folder call for and picks
/// among the machines that have it.
pub(super) async fn start_agent_session(cx: Cx, start: StartAgentSession) -> Reply<StartedAgent> {
    let prompt = start.prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(ApiError::bad_request("Say what the agent should do."));
    }
    let hosts = live_hosts(&cx).await?;
    if let Some(chat) = start.chat
        && !cx.user.owns_session(chat).await?
    {
        return Err(ApiError::NotFound {});
    }
    let (live, why) = match &start.machine_id {
        Some(id) => {
            let live = hosts
                .into_iter()
                .find(|live| &live.host.machine_id == id)
                .ok_or(ApiError::NotFound {})?;
            (live, String::new())
        }
        None => auto(hosts, &start, &prompt).await?,
    };
    let agent = live.agent(start.agent.as_deref())?;
    let cwd = start
        .cwd
        .map(|cwd| cwd.trim().to_owned())
        .filter(|cwd| !cwd.is_empty())
        .or_else(|| live.online.workspace.clone());
    let link = live
        .online
        .start_agent(&agent, cwd, prompt)
        .await
        .map_err(|e| {
            ApiError::bad_request(format!("Could not start {}: {e:#}", agent_name(&agent)))
        })?;
    let info = link.now().info;
    if let Some(chat) = start.chat {
        live.online.report_to(
            &info.id,
            Follower {
                user: cx.user.clone(),
                chat,
                node: command_name(&live.host.machine_name),
                agent,
            },
        );
    }
    Ok(StartedAgent {
        session: beep_session(&live.host(), info),
        why,
    })
}

/// The machine Auto picks for `start`, and why.
async fn auto(
    hosts: Vec<Live>,
    start: &StartAgentSession,
    prompt: &str,
) -> Result<(Live, String), ApiError> {
    let mut text = prompt.to_owned();
    if let Some(cwd) = &start.cwd {
        text.push('\n');
        text.push_str(cwd);
    }
    let needs = agent_auto::with(start.needs.clone(), agent_auto::called_for(&text));
    let offers = |live: &Live| {
        start
            .agent
            .as_deref()
            .is_none_or(|agent| live.offers(agent))
    };
    let mut candidates: Vec<Candidate<'_>> = hosts
        .iter()
        .map(|live| Candidate {
            name: &live.host.machine_name,
            offers: offers(live),
            capabilities: &live.host.capabilities,
            workspace: live.online.workspace.is_some(),
            running: 0,
        })
        .collect();
    let fit = agent_auto::fitting(&candidates, &needs);
    if agent_auto::needs_load(&candidates, &fit) {
        // The futures are made up front: a closure inside the stream would
        // keep the reply future from being `Send`.
        let asks: Vec<_> = fit
            .iter()
            .map(|&at| counted(at, hosts[at].online.clone()))
            .collect();
        let counts: Vec<(usize, u32)> = stream::iter(asks)
            .buffer_unordered(IN_FLIGHT)
            .collect()
            .await;
        for (at, count) in counts {
            candidates[at].running = count;
        }
    }
    let agent = start.agent.as_deref().map_or("a coding agent", agent_name);
    let choice =
        agent_auto::choose(&candidates, &fit, &needs, agent).map_err(ApiError::bad_request)?;
    let live = hosts
        .into_iter()
        .nth(choice.at)
        .ok_or(ApiError::Internal {})?;
    Ok((live, choice.why))
}

async fn counted(at: usize, online: Online) -> (usize, u32) {
    (at, running(&online).await)
}

/// Sessions that have not ended on `online`; a node that does not answer
/// counts as busy, so Auto goes elsewhere.
async fn running(online: &Online) -> u32 {
    match limited(online.agent_sessions()).await {
        Ok(sessions) => {
            let live = sessions
                .iter()
                .filter(|info| info.state.status != agent::Status::Ended)
                .count();
            u32::try_from(live).unwrap_or(u32::MAX)
        }
        Err(e) => {
            log::debug!("sessions not counted for Auto: {e:#}");
            u32::MAX
        }
    }
}

/// Admins only. A transcript that cannot be read leaves `entries` empty,
/// so the rest still shows.
pub(super) async fn get_agent_session(cx: Cx, get: GetAgentSession) -> Reply<AgentSessionView> {
    let live = live_host(&cx, &get.machine_id).await?;
    let host = live.host();
    let listed = limited(live.online.agent_sessions()).await?;
    let mut view = match listed
        .into_iter()
        .find(|info| info.id == get.session && info.agent == get.agent)
    {
        Some(info) => beep_view(&host, info),
        None => {
            // A node that cannot list outside sessions (an older one, or
            // one without the agent) has none to find.
            let outside = limited(live.online.external_sessions(
                &get.agent,
                1,
                Some(get.session.clone()),
            ))
            .await
            .unwrap_or_else(|e| {
                log::debug!("outside sessions not listed: {e:#}");
                Vec::new()
            });
            let found = outside
                .into_iter()
                .find(|session| session.id == get.session)
                .ok_or(ApiError::NotFound {})?;
            let last_message = found.last_message.clone();
            AgentSessionView {
                session: external_session(&host, found),
                plan: Vec::new(),
                files: Vec::new(),
                last_message,
                error: None,
                queued: 0,
                turns: 0,
                entries: Vec::new(),
                earlier: 0,
            }
        }
    };
    let last = get.last.clamp(1, MAX_ENTRIES);
    match timeout(NODE_LIMIT, live.online.transcript(&get.session, last)).await {
        Ok(Ok((entries, earlier))) => {
            view.entries = entries.into_iter().map(entry).collect();
            view.earlier = earlier;
        }
        Ok(Err(e)) => log::warn!("transcript not read; session={} error={e:#}", get.session),
        Err(_) => log::warn!("transcript not read in time; session={}", get.session),
    }
    Ok(view)
}

fn beep_view(host: &Host, info: SessionInfo) -> AgentSessionView {
    let state = info.state.clone();
    let last_message = info.last_message.clone();
    let turns = info.turns;
    AgentSessionView {
        session: beep_session(host, info),
        plan: state
            .plan
            .into_iter()
            .map(|step| AgentPlanStep {
                content: step.content,
                status: plan_status(step.status),
            })
            .collect(),
        files: state.files_touched,
        last_message,
        error: state.error,
        queued: state.queued,
        turns,
        entries: Vec::new(),
        earlier: 0,
    }
}

/// A status from a newer node reads as pending.
fn plan_status(status: PlanStatus) -> AgentPlanStatus {
    match status {
        PlanStatus::InProgress => AgentPlanStatus::InProgress {},
        PlanStatus::Completed => AgentPlanStatus::Completed {},
        _ => AgentPlanStatus::Pending {},
    }
}

fn entry(entry: agent::Entry) -> AgentEntry {
    let kind = match entry.kind {
        agent::EntryKind::User => AgentEntryKind::User {},
        agent::EntryKind::Agent => AgentEntryKind::Agent {},
        agent::EntryKind::Tool => AgentEntryKind::Tool {},
        agent::EntryKind::Question => AgentEntryKind::Question {},
        _ => AgentEntryKind::Other {},
    };
    AgentEntry {
        kind,
        text: entry.text,
    }
}

/// Admins only. Only sessions Beep runs take orders; one started outside
/// Beep is not found.
pub(super) async fn steer_agent_session(cx: Cx, steer: SteerAgentSession) -> Reply<()> {
    let live = live_host(&cx, &steer.machine_id).await?;
    let request = match steer.steer {
        AgentSteer::Prompt { text } if text.trim().is_empty() => {
            return Err(ApiError::bad_request("The message is empty."));
        }
        AgentSteer::Prompt { text } => Request::Prompt { text },
        AgentSteer::Cancel {} => Request::Cancel {},
        AgentSteer::Close {} => Request::Close {},
    };
    let listed = limited(live.online.agent_sessions()).await?;
    if !listed
        .iter()
        .any(|info| info.id == steer.session && info.agent == steer.agent)
    {
        return Err(ApiError::NotFound {});
    }
    let link = live
        .online
        .agent_session(&steer.agent, &steer.session)
        .await
        .map_err(|e| ApiError::bad_request(format!("Could not reach the session: {e:#}")))?;
    link.send(request)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(())
}
