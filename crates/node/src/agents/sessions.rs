//! The sessions this node runs. `Sessions` is the only code that changes the
//! map; it publishes each change as a new map, so readers take no lock.

use super::{Session, limits::MAX_KEPT};
use anyhow::Result;
use beep_proto::{
    SessionId,
    node::agent::Status,
    watched,
};
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::BuildHasher,
    sync::{Arc, Mutex, MutexGuard, Weak},
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

#[cfg(test)]
#[path = "../tests/agents_sessions.rs"]
mod tests;

/// Six hex digits, random enough to tell sessions apart.
pub(super) fn new_id() -> SessionId {
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    SessionId::new(format!("{:06x}", random & 0xff_ffff))
}
