//! Native registrations and the presence held by their open connections.

use beep_proto::SessionId;
use crate::{
    logs::{found::Agent, unix_now},
    lock,
};
use anyhow::{Result, bail};
use beep_proto::node::agent::{ExternalSession, Presence};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Mutex,
};
use tokio::sync::watch;
use uuid::Uuid;

const MAX_ACTIVE: usize = 256;
const MAX_ENDED: usize = 256;

#[derive(Debug, Deserialize)]
pub(crate) struct Registration {
    pub(crate) version: u32,
    pub(crate) agent: Agent,
    pub(crate) session_id: SessionId,
    pub(crate) cwd: String,
    #[serde(default)]
    pub(crate) pid: Option<u32>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) started_at: u64,
}

impl Registration {
    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.agent != Agent::Flsh {
            bail!("unsupported session presence protocol or agent");
        }

        if self.session_id.is_empty()
            || self.session_id.as_ref().len() > 256
            || self.session_id.as_ref().contains(['/', '\\', '\0'])
            || !Path::new(&self.cwd).is_absolute()
            || self.cwd.contains('\0')
            || self.pid == Some(0)
        {
            bail!("invalid session presence registration");
        }

        Ok(())
    }
}

struct Tracked {
    session: ExternalSession,
    connections: HashSet<Uuid>,
}

#[derive(Default)]
pub(crate) struct Registry {
    tracked: Mutex<HashMap<SessionId, Tracked>>,
    changes: watch::Sender<u64>,
}

impl Registry {
    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    pub(crate) fn register(&self, registration: Registration, connection: Uuid) -> Result<()> {
        registration.validate()?;
        let mut tracked = lock(&self.tracked);
        let exists = tracked.get(&registration.session_id);

        if !exists.is_some_and(|session| !session.connections.is_empty())
            && tracked
                .values()
                .filter(|s| !s.connections.is_empty())
                .count()
                >= MAX_ACTIVE
        {
            bail!("too many open sessions");
        }

        let now = unix_now();
        let session = tracked
            .entry(registration.session_id.clone())
            .or_insert_with(|| Tracked {
                session: ExternalSession {
                    id: registration.session_id,
                    agent: registration.agent.as_ref().to_owned(),
                    started_at: if registration.started_at == 0 {
                        now
                    } else {
                        registration.started_at.min(now)
                    },
                    ..ExternalSession::default()
                },
                connections: HashSet::new(),
            });
        session.connections.insert(connection);
        session.session.cwd = registration.cwd;
        session.session.pid = registration.pid;
        session.session.presence = Presence::Running;
        session.session.last_activity = now;
        if let Some(title) = registration.title {
            session.session.title = Some(crate::text::cut(&title, 300));
        }
        self.changes
            .send_modify(|count| *count = count.wrapping_add(1));
        crate::events::system().changed(beep_proto::node::events::Topic::new("agents"));
        Ok(())
    }

    pub(crate) fn close(&self, id: &SessionId, connection: Uuid) {
        let mut tracked = lock(&self.tracked);
        let Some(session) = tracked.get_mut(id) else {
            return;
        };

        if !session.connections.remove(&connection) || !session.connections.is_empty() {
            return;
        }

        session.session.presence = Presence::Stopped;
        session.session.pid = None;
        // Closed entries override recent transcript writes until evicted.
        if tracked
            .values()
            .filter(|s| s.connections.is_empty())
            .count()
            > MAX_ENDED
            && let Some(oldest) = tracked
                .iter()
                .filter(|(_, s)| s.connections.is_empty())
                .min_by_key(|(_, s)| s.session.last_activity)
                .map(|(id, _)| id.clone())
        {
            tracked.remove(&oldest);
        }

        self.changes
            .send_modify(|count| *count = count.wrapping_add(1));
        crate::events::system().changed(beep_proto::node::events::Topic::new("agents"));
    }

    pub(crate) fn snapshot(&self) -> Vec<ExternalSession> {
        let mut sessions: Vec<_> = lock(&self.tracked)
            .values()
            .map(|tracked| tracked.session.clone())
            .collect();
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        sessions
    }

    /// Presence overrides heuristics; transcript text and titles stay when
    /// the registration did not supply them. Open sessions need no log yet.
    pub(crate) fn merge(&self, sessions: &mut Vec<ExternalSession>) {
        for registered in self.snapshot() {
            if let Some(found) = sessions
                .iter_mut()
                .find(|found| found.agent == registered.agent && found.id == registered.id)
            {
                found.presence = registered.presence;
                found.pid = registered.pid;
                found.cwd = registered.cwd;
                if registered.title.is_some() {
                    found.title = registered.title;
                }
            } else {
                sessions.push(registered);
            }
        }
    }
}
