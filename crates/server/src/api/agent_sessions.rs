//! Coding-agent sessions on the user's machines, for the command center.

use super::{Reply, own_nodes::linked_keys};
use crate::{
    app::App,
    nodes::Online,
    protocol::{
        node::agent::{
            AnswerOption, ExternalSession, OptionKind, Presence, Question, Request,
            SESSIONS_PROTOCOL, SessionInfo, Status,
        },
        *,
    },
    router::Cx,
    user::User,
};
use anyhow::{Context, Result};
use futures::{StreamExt, future::join_all, stream};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::timeout;

/// How long one request to a node may take.
pub(super) const NODE_LIMIT: Duration = Duration::from_secs(3);
/// Nodes asked at once.
pub(super) const IN_FLIGHT: usize = 4;
/// The agents whose outside sessions are listed, and how many of each.
const EXTERNAL_AGENTS: [&str; 2] = ["claude", "codex"];
const EXTERNAL_LIMIT: u32 = 20;

/// The machine a session runs on.
#[derive(Clone)]
pub(super) struct Host {
    pub(super) id: String,
    pub(super) name: String,
}

pub(super) async fn list_agent_sessions(
    cx: Cx,
    _: ListAgentSessions,
) -> Reply<Vec<MachineSession>> {
    Ok(machine_sessions(&cx.app, &cx.user).await?)
}

/// The coding-agent sessions on `user`'s linked online machines, newest activity first.
pub(crate) async fn machine_sessions(
    app: &App,
    user: &User,
) -> anyhow::Result<Vec<MachineSession>> {
    let keys: HashSet<String> = user
        .account_keys()
        .await?
        .into_iter()
        .map(|key| key.endpoint_id)
        .collect();
    let names: HashMap<String, String> = user
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect();
    // Futures made up front: a closure inside the stream would make the reply not `Send`.
    let asks: Vec<_> = app
        .nodes
        .of_user(user.id())
        .into_iter()
        .filter_map(|online| {
            let id = online.connection.remote_id().to_string();
            if !keys.contains(&id) {
                return None;
            }
            let name = names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| online.name.clone());
            Some(node_sessions(online, Host { id, name }))
        })
        .collect();
    let mut sessions: Vec<MachineSession> = stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect();
    sessions.sort_by_key(|session| Reverse(session.last_activity));
    Ok(sessions)
}

/// Checks the question against the node's own list, so an answer that comes after the session moved on is refused.
pub(super) async fn answer_agent_question(cx: Cx, answer: AnswerAgentQuestion) -> Reply<()> {
    let keys = linked_keys(&cx).await?;
    let online = cx
        .app
        .nodes
        .online(&answer.machine_id)
        .filter(|online| online.user == cx.user.id() && keys.contains(&answer.machine_id))
        .ok_or(ApiError::NotFound {})?;
    let sessions = limited(online.agent_sessions()).await?;
    let session = sessions
        .into_iter()
        .find(|session| session.id == answer.session && session.agent == answer.agent)
        .ok_or(ApiError::NotFound {})?;
    if session.question.map(|question| question.id) != Some(answer.question) {
        return Err(ApiError::Conflict {
            message: "The session no longer asks that question.".into(),
        });
    }
    let link = limited(online.agent_session(&answer.agent, &answer.session)).await?;
    limited(link.send(Request::Answer {
        question: answer.question,
        reply: answer.reply,
    }))
    .await?;
    Ok(())
}

/// `call`, given up after `NODE_LIMIT`.
pub(super) async fn limited<T>(call: impl Future<Output = Result<T>>) -> Result<T> {
    timeout(NODE_LIMIT, call)
        .await
        .context("the node did not answer in time")?
}

/// The sessions on one node: none when its list fails.
async fn node_sessions(online: Online, host: Host) -> Vec<MachineSession> {
    if !online.offers(SESSIONS_PROTOCOL) {
        return Vec::new();
    }
    let external = join_all(EXTERNAL_AGENTS.map(|agent| external_sessions(&online, agent)));
    let (listed, external) = tokio::join!(limited(online.agent_sessions()), external);
    let listed = match listed {
        Ok(listed) => listed,
        Err(e) => {
            log::warn!("agent sessions not listed; node={} error={e:#}", host.id);
            return Vec::new();
        }
    };
    let beep = listed.into_iter().map(|info| beep_session(&host, info));
    let outside = external
        .into_iter()
        .flatten()
        .filter(|session| session.taken_over.is_none() && runs_now(session, SystemTime::now()))
        .map(|session| external_session(&host, session));
    beep.chain(outside).collect()
}

/// How long a session waiting for the user stays among those running now
/// after its last entry.
const IDLE_SHOWN: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether a session started outside Beep shows as running now: a process
/// runs it, and it is in a turn or was active within `IDLE_SHOWN`. A
/// terminal left open for days on an answered prompt is not.
fn runs_now(session: &ExternalSession, now: SystemTime) -> bool {
    match session.presence {
        Presence::Working | Presence::Running => true,
        Presence::Waiting => now
            .duration_since(unix(session.last_activity))
            .is_ok_and(|idle| idle < IDLE_SHOWN),
        _ => false,
    }
}

/// The sessions of `agent` started outside Beep; none when the node cannot list them.
async fn external_sessions(online: &Online, agent: &str) -> Vec<ExternalSession> {
    match limited(online.external_sessions(agent, EXTERNAL_LIMIT, None)).await {
        Ok(sessions) => sessions,
        Err(e) => {
            log::debug!("external {agent} sessions not listed: {e:#}");
            Vec::new()
        }
    }
}

/// A session Beep started; its `last_activity` is its start, as the node reports no later time.
pub(super) fn beep_session(host: &Host, info: SessionInfo) -> MachineSession {
    let started_at = unix(info.started_at);
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: info.id,
        agent: info.agent,
        title: info.title,
        cwd: info.cwd,
        status: agent_status(info.state.status),
        doing: Some(info.state.summary).filter(|summary| !summary.is_empty()),
        started_at,
        last_activity: started_at,
        external: false,
        question: info.question.map(agent_question),
    }
}

pub(super) fn external_session(host: &Host, session: ExternalSession) -> MachineSession {
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: session.id,
        agent: session.agent,
        title: session.title,
        cwd: session.cwd,
        status: presence_status(session.presence),
        doing: None,
        started_at: unix(session.started_at),
        last_activity: unix(session.last_activity),
        external: true,
        question: None,
    }
}

/// A status from a newer node reads as working.
fn agent_status(status: Status) -> AgentStatus {
    match status {
        Status::Starting => AgentStatus::Starting {},
        Status::Asking => AgentStatus::Asking {},
        Status::Idle => AgentStatus::Idle {},
        Status::Ended => AgentStatus::Ended {},
        _ => AgentStatus::Working {},
    }
}

/// Only live presences are listed; any other reads as running.
fn presence_status(presence: Presence) -> AgentStatus {
    match presence {
        Presence::Working => AgentStatus::Working {},
        Presence::Waiting => AgentStatus::Waiting {},
        _ => AgentStatus::Running {},
    }
}

fn agent_question(question: Question) -> AgentQuestion {
    AgentQuestion {
        id: question.id,
        text: question.text,
        detail: question.detail,
        options: question.options.into_iter().map(agent_option).collect(),
    }
}

fn agent_option(option: AnswerOption) -> AgentOption {
    AgentOption {
        id: option.id,
        name: option.name,
        allow: matches!(option.kind, OptionKind::AllowOnce | OptionKind::AllowAlways),
    }
}

fn unix(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_idle_for_a_day_is_not_running_now() {
        let now = unix(10 * 24 * 60 * 60);
        let session = |presence, hours_ago: u64| ExternalSession {
            presence,
            last_activity: 10 * 24 * 60 * 60 - hours_ago * 60 * 60,
            ..ExternalSession::default()
        };
        assert!(runs_now(&session(Presence::Waiting, 2), now));
        assert!(!runs_now(&session(Presence::Waiting, 25), now));
        // A turn in progress shows however old its last entry.
        assert!(runs_now(&session(Presence::Working, 200), now));
        assert!(!runs_now(&session(Presence::Stopped, 0), now));
    }
}
