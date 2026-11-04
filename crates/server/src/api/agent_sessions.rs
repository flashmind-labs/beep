//! Coding-agent sessions on the user's machines, for the command center.

use super::{Reply, own_nodes::linked_keys};
use crate::{
    account::user::User,
    api::router::Cx,
    app::App,
    protocol::{node::agent::Request, *},
};
use anyhow::{Context, Result};
use futures::{StreamExt, stream};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    future::Future,
};
use tokio::time::timeout;

/// Nodes asked at once.
pub(super) const IN_FLIGHT: usize = 4;

pub(super) use beep_mesh::machine::{
    Host, beep_session, external_session,
    listing::{self, NODE_LIMIT},
};

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
            Some(async move { listing::node_sessions(&online, &Host { id, name }).await })
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
