//! Coding-agent sessions on a node: one followed stream per session, keeping its latest state.

mod follow;
mod online;
use follow::{Failed, Failure, Opening, Retry, follow};

use super::{Ask, Follower};
use crate::{
    nodes::READ_TIMEOUT,
    protocol::node::agent::{Question, Request, SessionInfo, Status, StopReason},
};
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;

pub(crate) use wake::News;

mod wake;

/// The node refused the session stream's first request (for an attach: it lost the session).
#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub(crate) struct SessionRefused {
    pub(crate) reason: String,
}

pub use beep_mesh::machine::StartOn;
pub(crate) use beep_mesh::machine::{NodeFile, NodeImage, NodeWork, WorkRead, WorkWrite};

/// Unreadable messages in a row before a link gives up.
const MAX_BAD_MESSAGES: u32 = 8;
const ORDERS: usize = 8;

/// The most sessions on one node that report to a chat.
const MAX_OWNERS: usize = 64;

/// The sessions a node's links follow, by id, and the chats they report to.
#[derive(Default)]
pub struct AgentLinks {
    links: Mutex<HashMap<String, Arc<AgentLink>>>,
    owners: Mutex<HashMap<String, Follower>>,
}

impl AgentLinks {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<AgentLink>>> {
        self.links.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn owners(&self) -> MutexGuard<'_, HashMap<String, Follower>> {
        self.owners.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The chat `session` reports to.
    pub(super) fn owner(&self, session: &str) -> Option<Follower> {
        self.owners().get(session).cloned()
    }

    fn set_owner(&self, session: &str, owner: Follower) {
        let mut owners = self.owners();
        if !owners.contains_key(session) && owners.len() >= MAX_OWNERS {
            // Sessions that ended or went away keep their place until here.
            let links = self.lock();
            owners.retain(|id, _| links.get(id).is_some_and(|link| !link.is_lost()));
        }
        if owners.contains_key(session) || owners.len() < MAX_OWNERS {
            owners.insert(session.to_owned(), owner);
        } else {
            log::warn!("nodes: too many sessions report to chats; {session} will not");
        }
    }

    fn live(&self, session: &str) -> Option<Arc<AgentLink>> {
        let mut links = self.lock();
        match links.get(session) {
            Some(link) if !link.is_lost() => Some(link.clone()),
            Some(_) => {
                links.remove(session);
                None
            }
            None => None,
        }
    }
}

/// A session as the server last heard of it.
#[derive(Debug, Clone, Default)]
pub struct Watched {
    pub info: SessionInfo,
    pub last_stop: Option<StopReason>,
    /// Why the session ended, once it has.
    pub ended: Option<String>,
    /// The stream dropped; the session may still run on the node.
    pub lost: bool,
}

/// Where a session was before an order, to wait for what comes after.
#[derive(Debug, Clone, Copy)]
pub struct Mark {
    turns: u32,
    question: Option<u32>,
}

impl Watched {
    pub fn mark(&self) -> Mark {
        Mark {
            turns: self.info.turns,
            question: self.info.question.as_ref().map(|question| question.id),
        }
    }

    /// A turn ended, a new question came, or the session is gone since `mark`.
    fn moved_on(&self, mark: Mark) -> bool {
        let question = self.info.question.as_ref().map(|question| question.id);
        self.ended.is_some()
            || self.lost
            || self.info.turns > mark.turns
            || (question.is_some() && question != mark.question)
    }

    /// Nothing will happen until someone acts.
    pub fn settled(&self) -> bool {
        self.ended.is_some()
            || self.lost
            || matches!(
                self.info.state.status,
                Status::Idle | Status::Asking | Status::Ended
            )
    }
}

/// One followed session.
pub struct AgentLink {
    orders: mpsc::Sender<LinkOrder>,
    state: watch::Receiver<Watched>,
    /// Calls waiting for the session; while one does, no report goes to the chat.
    waiting: Arc<AtomicUsize>,
    _task: AbortOnDropHandle<()>,
}

struct LinkOrder {
    request: Request,
    done: oneshot::Sender<Result<(), String>>,
}

impl AgentLink {
    pub fn now(&self) -> Watched {
        self.state.borrow().clone()
    }

    /// The card for the session's open `question` in `follower`'s chat.
    pub(crate) fn ask(&self, follower: Follower, question: Question, task: String) -> Ask {
        let session = self.state.borrow().info.id.clone();
        wake::ask_card(
            &self.state,
            &self.orders,
            session,
            follower,
            question,
            Some(task),
        )
    }

    fn is_lost(&self) -> bool {
        let state = self.state.borrow();
        state.lost || state.ended.is_some()
    }

    /// Sends `request` and waits for the node to take it; cancelling leaves the stream whole.
    pub async fn send(&self, request: Request) -> Result<()> {
        order(&self.orders, request).await
    }

    /// Waits up to `limit` for the session to move on from `mark`, then gives its state.
    pub async fn wait(&self, mark: Mark, limit: Duration) -> Watched {
        let _waiting = Waiting::new(&self.waiting);
        let mut state = self.state.clone();
        // Timing out or losing the task both leave the latest state.
        let _ = timeout(limit, state.wait_for(|now| now.moved_on(mark))).await;
        self.now()
    }
}

/// Counts a call waiting for a session while it lives.
struct Waiting(Arc<AtomicUsize>);

impl Waiting {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count.clone())
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Sends `request` down a session's stream and waits for the node to take it.
async fn order(orders: &mpsc::Sender<LinkOrder>, request: Request) -> Result<()> {
    let (done, answer) = oneshot::channel();
    orders
        .send(LinkOrder { request, done })
        .await
        .map_err(|_| anyhow::anyhow!("lost the session's stream"))?;
    let taken = timeout(READ_TIMEOUT, answer)
        .await
        .context("the node did not answer")?
        .map_err(|_| anyhow::anyhow!("lost the session's stream"))?;
    taken.map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests;
