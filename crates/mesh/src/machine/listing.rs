//! A node's sessions as the apps list them.

use super::{
    Host, beep_session, external_session, past_session, runs_now,
    sessions::{self, Sessions},
};
use beep_proto::{MachineSession, node::agent::ParentLink};
use futures::future::join_all;
use std::time::{Duration, Instant, SystemTime};
use tokio::time::timeout;

/// How long one request to a node may take.
pub const NODE_LIMIT: Duration = Duration::from_secs(3);
/// The agents whose outside sessions are listed, and how many of each.
const EXTERNAL_AGENTS: [&str; 2] = ["claude", "codex"];
const EXTERNAL_LIMIT: u32 = 20;
/// The most sessions a project lists.
pub const MAX_PROJECT_SESSIONS: usize = 50;
/// A node slower than this to list a project's sessions is logged.
const SLOW_NODE: Duration = Duration::from_secs(1);

/// The sessions on one node, Beep's and those started outside it that run
/// now: none when its list fails. Beep's wait for the outside scan; a caller
/// that cannot wait asks for each half apart.
pub async fn node_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<MachineSession> {
    let (mut beep, outside) = tokio::join!(beep_sessions(node, host), outside_sessions(node, host));
    beep.extend(outside);
    beep
}

/// The sessions Beep runs on one node: none when its list fails.
pub async fn beep_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<MachineSession> {
    linked_sessions(node, host)
        .await
        .into_iter()
        .map(|linked| linked.session)
        .collect()
}

/// A session Beep runs, and the session that started it when an agent did.
#[derive(Debug, Clone)]
pub struct Linked {
    pub session: MachineSession,
    pub parent: Option<ParentLink>,
}

/// `beep_sessions`, with who started each: what an app needs to nest a
/// child under its parent. `MachineSession` stays as it is for the server's
/// apps.
pub async fn linked_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<Linked> {
    match timeout(NODE_LIMIT, sessions::list(node)).await {
        Ok(Ok(listed)) => listed
            .into_iter()
            .map(|info| Linked {
                parent: info.parent.clone(),
                session: beep_session(host, info),
            })
            .collect(),
        Ok(Err(e)) => {
            tracing::warn!("agent sessions not listed; node={} error={e:#}", host.id);
            Vec::new()
        }
        Err(_) => {
            tracing::warn!("agent sessions not listed in time; node={}", host.id);
            Vec::new()
        }
    }
}

/// The sessions started outside Beep that run now, from scanning what each
/// agent keeps on the node. A scan takes seconds.
pub async fn outside_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<MachineSession> {
    join_all(EXTERNAL_AGENTS.map(|agent| outside(node, agent)))
        .await
        .into_iter()
        .flatten()
        .filter(|session| session.taken_over.is_none() && runs_now(session, SystemTime::now()))
        .map(|session| external_session(host, session))
        .collect()
}

/// The sessions of `agent` started outside Beep; none when the node cannot list them.
async fn outside<S: Sessions + ?Sized>(
    node: &S,
    agent: &str,
) -> Vec<beep_proto::node::agent::ExternalSession> {
    match timeout(
        NODE_LIMIT,
        sessions::external(node, agent, EXTERNAL_LIMIT, None),
    )
    .await
    {
        Ok(Ok(sessions)) => sessions,
        _ => Vec::new(),
    }
}

/// The sessions coding agents ran in `folders` on one node, as a project lists them.
pub async fn project_sessions<S: Sessions + ?Sized>(
    node: &S,
    host: &Host,
    folders: Vec<String>,
) -> Vec<MachineSession> {
    let limit = u32::try_from(MAX_PROJECT_SESSIONS).unwrap_or(u32::MAX);
    let started = Instant::now();
    let listed = timeout(NODE_LIMIT, sessions::in_folders(node, folders, limit)).await;
    if started.elapsed() >= SLOW_NODE {
        tracing::info!(
            "project sessions listed slowly; node={} ms={}",
            host.id,
            started.elapsed().as_millis()
        );
    }
    match listed {
        Ok(Ok(found)) => found
            .into_iter()
            .map(|session| past_session(host, session))
            .collect(),
        Ok(Err(e)) => {
            tracing::info!("project sessions not listed; node={} error={e:#}", host.id);
            Vec::new()
        }
        Err(_) => {
            tracing::info!("project sessions not listed in time; node={}", host.id);
            Vec::new()
        }
    }
}
