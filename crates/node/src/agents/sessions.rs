//! The sessions this node runs. One task owns the map and is its only
//! writer; it publishes each change as a new map, so readers take no lock and
//! send no message.

use super::{MAX_KEPT, Session, running::new_id};
use anyhow::{Result, anyhow};
use beep_proto::{
    SessionId,
    node::agent::Status,
    watched,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock, Weak},
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::task::AbortOnDropHandle;

const WAITING: usize = 64;
const STOPPED: &str = "the sessions' supervisor stopped";

pub(super) type Map = HashMap<SessionId, Arc<Session>>;

/// The node's sessions. The task that owns them starts with the first
/// command and ends with this.
#[derive(Default)]
pub(super) struct Sessions {
    map: watch::Sender<Arc<Map>>,
    started: OnceLock<Started>,
}

struct Started {
    orders: mpsc::Sender<Order>,
    _task: AbortOnDropHandle<()>,
}

/// A start that has its id and no session yet. Dropped, it gives the id
/// back.
pub(super) struct Reservation {
    pub(super) id: SessionId,
    _held: Arc<()>,
}

enum Order {
    /// Picks the id of a start and refuses a second takeover of one session.
    Reserve {
        resume: Option<SessionId>,
        cwd: PathBuf,
        reply: oneshot::Sender<Result<Reservation, String>>,
    },
    /// Adds the session a reservation was for.
    Insert {
        id: SessionId,
        resume: Option<SessionId>,
        session: Arc<Session>,
        reply: oneshot::Sender<()>,
    },
    Remove {
        ids: Vec<SessionId>,
        reply: oneshot::Sender<()>,
    },
    /// The folders of the starts that have no session yet.
    Starting { reply: oneshot::Sender<Vec<PathBuf>> },
}

impl Sessions {
    /// The sessions now. A later change does not show in it.
    pub(super) fn read(&self) -> Arc<Map> {
        watched::read_sent(&self.map, Arc::clone)
    }

    /// Takes the id for a session started in `cwd`, or the agent's `resume`
    /// id of one taken over. Refused while another run of that session is
    /// live or starting.
    pub(super) async fn reserve(
        &self,
        resume: Option<SessionId>,
        cwd: PathBuf,
    ) -> Result<Reservation> {
        self.ask(|reply| Order::Reserve { resume, cwd, reply })
            .await?
            .map_err(anyhow::Error::msg)
    }

    /// Adds `session` for `reservation`. Once this returns, `read` shows
    /// it. A resumed session replaces the ended runs of it, and a full map
    /// drops its oldest ended session.
    pub(super) async fn insert(
        &self,
        reservation: Reservation,
        resume: Option<SessionId>,
        session: Arc<Session>,
    ) -> Result<()> {
        self.ask(|reply| Order::Insert {
            id: reservation.id,
            resume,
            session,
            reply,
        })
        .await
    }

    /// Takes the sessions `ids` out. Once this returns, `read` shows none.
    pub(super) async fn remove(&self, ids: Vec<SessionId>) {
        if self.ask(|reply| Order::Remove { ids, reply }).await.is_err() {
            log::warn!("{STOPPED}");
        }
    }

    /// The folders of the starts under way.
    pub(super) async fn starting(&self) -> Result<Vec<PathBuf>> {
        self.ask(|reply| Order::Starting { reply }).await
    }

    async fn ask<R>(&self, order: impl FnOnce(oneshot::Sender<R>) -> Order) -> Result<R> {
        let started = self.started.get_or_init(|| {
            let (orders, ordered) = mpsc::channel(WAITING);
            Started {
                orders,
                _task: AbortOnDropHandle::new(tokio::spawn(supervise(
                    self.map.clone(),
                    ordered,
                ))),
            }
        });
        let (reply, answer) = oneshot::channel();
        started
            .orders
            .send(order(reply))
            .await
            .map_err(|_| anyhow!(STOPPED))?;

        answer.await.map_err(|_| anyhow!(STOPPED))
    }
}

/// A start that holds an id.
struct Pending {
    id: SessionId,
    cwd: PathBuf,
    held: Weak<()>,
}

async fn supervise(map: watch::Sender<Arc<Map>>, mut orders: mpsc::Receiver<Order>) {
    let mut pending: Vec<Pending> = Vec::new();

    while let Some(order) = orders.recv().await {
        // A start that was dropped before it inserted gives its id back.
        pending.retain(|start| start.held.strong_count() > 0);

        match order {
            Order::Reserve { resume, cwd, reply } => {
                let _ = reply.send(reserve(&map, &mut pending, resume, cwd));
            }
            Order::Insert {
                id,
                resume,
                session,
                reply,
            } => {
                pending.retain(|start| start.id != id);
                map.send_modify(|sessions| insert(Arc::make_mut(sessions), id, resume, session));
                let _ = reply.send(());
            }
            Order::Remove { ids, reply } => {
                map.send_if_modified(|sessions| {
                    if !ids.iter().any(|id| sessions.contains_key(id)) {
                        return false;
                    }

                    let sessions = Arc::make_mut(sessions);
                    ids.iter().for_each(|id| drop(sessions.remove(id)));
                    true
                });
                let _ = reply.send(());
            }
            Order::Starting { reply } => {
                let _ = reply.send(pending.iter().map(|start| start.cwd.clone()).collect());
            }
        }
    }
}

fn reserve(
    map: &watch::Sender<Arc<Map>>,
    pending: &mut Vec<Pending>,
    resume: Option<SessionId>,
    cwd: PathBuf,
) -> Result<Reservation, String> {
    let sessions = watched::read_sent(map, Arc::clone);
    let id = match &resume {
        Some(resume) => {
            // Two takeovers of one session must not both start it.
            if let Some(running) = sessions.values().find(|session| {
                watched::read(&session.live, |live| {
                    live.status() != Status::Ended && live.agent_session.as_ref() == Some(resume)
                })
            }) {
                return Err(format!("session {resume} already runs here as {}", running.id));
            }

            if pending.iter().any(|start| start.id == *resume) {
                return Err(format!("session {resume} already runs here as {resume}"));
            }

            resume.clone()
        }
        None => loop {
            let id = new_id();

            if !sessions.contains_key(&id) && pending.iter().all(|start| start.id != id) {
                break id;
            }
        },
    };
    let held = Arc::new(());
    pending.push(Pending {
        id: id.clone(),
        cwd,
        held: Arc::downgrade(&held),
    });

    Ok(Reservation { id, _held: held })
}

fn insert(
    sessions: &mut Map,
    id: SessionId,
    resume: Option<SessionId>,
    session: Arc<Session>,
) {
    if sessions.len() >= MAX_KEPT {
        let oldest = sessions
            .values()
            .filter(|session| watched::read(&session.live, |live| live.status() == Status::Ended))
            .min_by_key(|session| watched::read(&session.live, |live| live.info.started_at))
            .map(|session| session.id.clone());

        if let Some(oldest) = oldest {
            sessions.remove(&oldest);
        }
    }

    if let Some(resume) = resume {
        // The ended run may be keyed by its original private handle.
        // Remove it too, so listings cannot shadow the resumed run.
        sessions.retain(|handle, session| {
            *handle != resume
                && watched::read(&session.live, |live| live.agent_session.as_ref() != Some(&resume))
        });
    }

    sessions.insert(id, session);
}

#[cfg(test)]
#[path = "../tests/agents_sessions.rs"]
mod tests;
