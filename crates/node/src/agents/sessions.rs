//! The sessions this node runs. `Sessions` is the only code that changes the
//! map; it publishes each change as a new map, so readers take no lock.

use super::{Session, family::links::{Who, is_child_of}, limits::MAX_KEPT};
use crate::AsyncMutex;
use anyhow::Result;
use beep_proto::{
    SessionId,
    node::agent::{SessionInfo, Status},
    watched,
};
use std::{
    collections::{HashMap, HashSet, hash_map::RandomState},
    hash::BuildHasher,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, Weak, atomic::AtomicBool},
};
use tokio::sync::watch;

pub(crate) type Map = HashMap<SessionId, Arc<Session>>;

/// The node's sessions.
#[derive(Default)]
pub(crate) struct Sessions {
    map: watch::Sender<Arc<Map>>,
    /// The starts that hold an id and have no session yet. Locked for each
    /// reservation and insert, so an id is checked and taken in one step.
    pending: Mutex<Vec<Pending>>,
    /// Serializes changes to a stored session against resuming it, one
    /// lock for each session by the agent's own id; see `store_of`.
    pub(super) store: Mutex<HashMap<SessionId, Weak<AsyncMutex<()>>>>,
    /// Set once the node is stopping.
    pub(super) stopping: Arc<AtomicBool>,
}

/// A start that has its id and no session yet. Dropped, it gives the id
/// back.
pub(super) struct Reservation {
    pub(super) id: SessionId,
    _held: Arc<()>,
}

/// A start that holds an id.
struct Pending {
    id: SessionId,
    held: Weak<()>,
}

impl Sessions {
    /// The sessions now. A later change does not show in it.
    pub(super) fn read(&self) -> Arc<Map> {
        watched::read_sent(&self.map, Arc::clone)
    }

    /// The map as it changes, for what follows the sessions.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Arc<Map>> {
        self.map.subscribe()
    }

    /// Takes the id for a new session, or the agent's `resume`
    /// id of one taken over. Refused while another run of that session is
    /// live or starting.
    pub(super) fn reserve(&self, resume: Option<SessionId>) -> Result<Reservation> {
        let (id, held) = reserve(&self.map, &mut self.pending(), resume, new_id)
            .map_err(anyhow::Error::msg)?;

        Ok(Reservation { id, _held: held })
    }

    /// Adds `session` for `reservation`. A resumed session replaces the
    /// ended runs of it, and a full map drops its oldest ended session.
    pub(super) fn insert(
        &self,
        reservation: Reservation,
        resume: Option<SessionId>,
        session: Arc<Session>,
    ) {
        // Held until the session is in the map, so its id is never free
        // between, and two inserts do not push out the same session.
        let mut pending = self.pending();
        pending.retain(|start| start.id != reservation.id);
        let doomed = doomed(&self.read(), resume.as_ref());

        self.map.send_modify(|sessions| {
            let sessions = Arc::make_mut(sessions);
            doomed.iter().for_each(|id| drop(sessions.remove(id)));
            sessions.insert(reservation.id.clone(), session);
        });
    }

    /// Takes the sessions `ids` out.
    pub(super) fn remove(&self, ids: &[SessionId]) {
        self.map.send_if_modified(|sessions| {
            if !ids.iter().any(|id| sessions.contains_key(id)) {
                return false;
            }

            let sessions = Arc::make_mut(sessions);
            ids.iter().for_each(|id| drop(sessions.remove(id)));
            true
        });
    }

    /// Every named session but the reviewers, oldest first.
    pub(super) fn list(&self) -> Vec<SessionInfo> {
        self.listed(false)
    }

    /// `list`, with the reviewers that run as a child when `reviewers`.
    pub(super) fn listed(&self, reviewers: bool) -> Vec<SessionInfo> {
        self.listed_where(reviewers, None)
    }

    pub(super) fn listed_where(&self, reviewers: bool, id: Option<&SessionId>) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self
            .read()
            .values()
            .filter_map(|s| {
                // One read: a second while this one is held waits behind any
                // write queued in between, which waits on this one, and the
                // node stops.
                let info = watched::read(&s.live, |live| {
                    (live.agent_session.is_some()
                        && id.is_none_or(|id| live.info.id == *id)
                        && (!s.reviewer || reviewers && live.info.parent.is_some()))
                    .then(|| live.info.clone())
                })?;

                Some(s.listed(info))
            })
            .collect();
        sessions.sort_by_key(|info| info.started_at);
        sessions
    }

    /// The folders this node's sessions work in, the ended ones it keeps too.
    pub(super) fn folders(&self) -> std::collections::BTreeSet<PathBuf> {
        let sessions: Vec<Arc<Session>> = self.read().values().cloned().collect();

        sessions
            .iter()
            .map(|session| PathBuf::from(session.info().cwd))
            .collect()
    }

    /// The sessions this node runs, by the agent's own id, to the node's id.
    pub(super) fn owned(&self) -> HashMap<SessionId, SessionId> {
        self.read()
            .values()
            .filter_map(|session| {
                watched::read(&session.live, |live| {
                    Some((live.agent_session.clone()?, live.info.id.clone()))
                })
            })
            .collect()
    }

    /// The root pids of sessions this node is starting before ACP names them.
    pub(super) fn starting_pids(&self) -> HashSet<u32> {
        self.read()
            .values()
            .filter_map(|session| {
                watched::read(&session.live, |live| {
                    (live.status() != Status::Ended && live.agent_session.is_none())
                        .then_some(live.root_pid)
                        .flatten()
                })
            })
            .collect()
    }

    /// The lock that keeps a resume, a delete and a move of one stored
    /// session from interleaving. It is per session, by the agent's own id
    /// when the session is here, so two sessions never wait on each other.
    /// It lasts while a holder keeps the handle.
    pub(super) fn store_of(&self, id: &SessionId) -> Arc<AsyncMutex<()>> {
        let key = find_session(&self.read(), id)
            .and_then(|session| watched::read(&session.live, |live| live.agent_session.clone()))
            .unwrap_or_else(|| id.clone());
        let mut held = crate::lock(&self.store);
        held.retain(|_, lock| lock.strong_count() > 0);

        if let Some(lock) = held.get(&key).and_then(std::sync::Weak::upgrade) {
            return lock;
        }

        let lock = Arc::new(AsyncMutex::new(()));
        held.insert(key, Arc::downgrade(&lock));

        lock
    }

    /// The live sessions here that other agents may list and message: every
    /// one the user sees, oldest first; with those that ended and were last
    /// active at or after `ended_since`, Unix seconds, when given.
    pub(super) fn peers(&self, ended_since: Option<u64>) -> Vec<Arc<Session>> {
        let mut peers: Vec<Arc<Session>> = self
            .read()
            .values()
            .filter(|session| {
                !session.reviewer
                    && watched::read(&session.live, |live| {
                        let shown = match live.status() {
                            Status::Ended => ended_since
                                .is_some_and(|since| live.info.last_activity >= since),
                            _ => true,
                        };
                        live.agent_session.is_some() && shown
                    })
            })
            .cloned()
            .collect();
        peers.sort_by_key(|session| watched::read(&session.live, |live| live.info.started_at));
        peers
    }

    /// The live children of `who` that run here.
    pub(super) fn local_children(&self, who: &Who) -> usize {
        self.read()
            .values()
            .filter(|session| {
                watched::read(&session.live, |live| {
                    is_child_of(live, who) && live.status() != Status::Ended
                })
            })
            .count()
    }

    /// The starts under way. One that was dropped before it inserted gives
    /// its id back here.
    fn pending(&self) -> MutexGuard<'_, Vec<Pending>> {
        let mut pending = crate::lock(&self.pending);
        pending.retain(|start| start.held.strong_count() > 0);
        pending
    }
}

fn reserve(
    map: &watch::Sender<Arc<Map>>,
    pending: &mut Vec<Pending>,
    resume: Option<SessionId>,
    mut new_id: impl FnMut() -> SessionId,
) -> Result<(SessionId, Arc<()>), String> {
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
        held: Arc::downgrade(&held),
    });

    Ok((id, held))
}

/// The sessions a new one pushes out of `sessions`: the oldest ended one
/// when the map is full, and the ended runs of one taken over, which
/// listings must not show beside it.
fn doomed(sessions: &Map, resume: Option<&SessionId>) -> Vec<SessionId> {
    let mut doomed = Vec::new();

    if sessions.len() >= MAX_KEPT {
        let oldest = sessions
            .values()
            .filter(|session| watched::read(&session.live, |live| live.status() == Status::Ended))
            .min_by_key(|session| watched::read(&session.live, |live| live.info.started_at));

        doomed.extend(oldest.map(|session| session.id.clone()));
    }

    if let Some(resume) = resume {
        // The ended run may be keyed by its original private handle.
        doomed.extend(
            sessions
                .iter()
                .filter(|(handle, session)| {
                    *handle == resume
                        || watched::read(&session.live, |live| {
                            live.agent_session.as_ref() == Some(resume)
                        })
                })
                .map(|(handle, _)| handle.clone()),
        );
    }

    doomed
}

/// Six hex digits, random enough to tell sessions apart.
pub(super) fn new_id() -> SessionId {
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    SessionId::new(format!("{:06x}", random & 0xff_ffff))
}

/// The session `id` names in `sessions`, by its key or the agent's id.
pub(super) fn find_session(
    sessions: &Map,
    id: &SessionId,
) -> Option<Arc<Session>> {
    if let Some(found) = sessions.get(id) {
        return Some(found.clone());
    }

    sessions
        .values()
        .filter(|session| {
            watched::read(&session.live, |live| {
                live.agent_session.as_ref() == Some(id)
            })
        })
        .max_by_key(|session| {
            watched::read(&session.live, |live| {
                (live.status() != Status::Ended, live.info.started_at)
            })
        })
        .cloned()
}

#[cfg(test)]
#[path = "../tests/agents_sessions.rs"]
mod tests;
