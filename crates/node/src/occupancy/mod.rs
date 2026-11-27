//! Who works in which folder on this machine (`Occupancy`): the folders
//! sessions run in, the checkouts sync holds, the sessions a CLI holds open
//! and what Claude Code's hooks report, with the one read of the agents' logs
//! that lists the sessions outside Beep. Sync and the agents both ask it;
//! it asks neither.

pub mod claude;
pub mod holds;
mod holders;
pub(crate) mod presence;
mod read;
mod reserve;
mod sighted;

pub(crate) use holders::{Holding, settle};
pub(crate) use sighted::Watching;

use crate::{
    logs::{claude::Store, found::{Agent, Stores}},
    lock,
};
use anyhow::{Result, bail};
use beep_proto::{
    SessionId,
    node::agent::{ExternalSession, LiveStatus, Presence, Steer},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;

pub use holds::{MAX_IN_FOLDERS, RepositoryHold, RepositoryHolds, Running};

/// How long a session `close_external` ended reads as stopped despite a
/// recent write: as long as such a write counts.
const CLOSED_FOR: Duration = Duration::from_secs(3 * 60);

/// The occupancy of this machine. Clones share it.
#[derive(Clone, Default)]
pub struct Occupancy(Arc<Inner>);

#[derive(Default)]
struct Inner {
    /// The checkouts held for Git recovery and the folders sessions run in.
    holds: RepositoryHolds,
    /// Sessions held open by a local CLI, including idle prompts.
    presence: presence::Registry,
    /// What Claude Code's hooks report.
    hooks: claude::Hooks,
    /// Which processes hold the sessions' files, checked once for all.
    holders: holders::Holders,
    /// Sessions started outside Beep whose process this node ended or saw
    /// exit lately, by id, with when.
    closed: Mutex<HashMap<SessionId, Instant>>,
    /// The one read of every store, and who waits for it.
    reading: Arc<sighted::Reading>,
    /// Where Claude Code keeps its sessions.
    claude: Store,
    /// Where Codex, pi, flsh and OpenCode keep theirs.
    stores: Stores,
}

impl Occupancy {
    /// The stores of the agents on this machine.
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            claude: Store::locate(),
            stores: Stores::locate(),
            ..Inner::default()
        }))
    }

    /// An occupancy over the given stores.
    #[cfg(test)]
    pub(crate) fn over(claude: Store, stores: Stores) -> Self {
        Self(Arc::new(Inner {
            claude,
            stores,
            ..Inner::default()
        }))
    }

    pub(crate) fn holds(&self) -> &RepositoryHolds {
        &self.0.holds
    }

    pub(crate) fn presence(&self) -> &presence::Registry {
        &self.0.presence
    }

    pub(crate) fn claude(&self) -> &Store {
        &self.0.claude
    }

    pub(crate) fn stores(&self) -> &Stores {
        &self.0.stores
    }

    /// The hook events from now on.
    pub(crate) fn hook_events(&self) -> tokio::sync::broadcast::Receiver<beep_proto::node::agent::Activity> {
        self.0.hooks.hub.subscribe()
    }

    /// Takes hook events on the socket in `dir` until the node stops.
    pub async fn listen_hooks(&self, dir: &Path) -> Result<()> {
        claude::listen(&claude::hooks::socket_path(dir), &self.0.hooks.hub, &self.0.hooks.live).await
    }

    /// Takes native session presence for the user, for a node embedded in
    /// the desktop.
    pub async fn listen_presence(&self, dir: &Path) -> Result<()> {
        presence::listen(self.clone(), dir).await
    }

    /// As `listen_presence`, for a standalone node in `dir`.
    pub async fn listen_presence_in(&self, dir: &Path) -> Result<()> {
        presence::listen_in(self.clone(), dir).await
    }

    /// How session `id` does now as its hooks report it, after queuing
    /// `steer` for its next hook when given. `owned` maps the sessions this
    /// node runs, by Claude Code's id, to the node's id.
    pub async fn live(
        &self,
        id: &SessionId,
        steer: Option<Steer>,
        owned: HashMap<SessionId, SessionId>,
    ) -> Result<LiveStatus> {
        let hooks = &self.0.hooks;
        let found = match self.claude().list(1, Some(id.clone()), owned).await?.pop() {
            Some(found) => found,
            None if hooks.live.knows(id) => ExternalSession {
                id: id.clone(),
                agent: "claude".to_owned(),
                ..ExternalSession::default()
            },
            None => bail!("there is no Claude Code session {id} here"),
        };

        if let Some(taken) = &found.taken_over {
            bail!("session {id} runs here as session {taken}, so steer it as that session");
        }

        if let Some(steer) = steer {
            hooks.live.steer(id, steer).map_err(anyhow::Error::msg)?;
        }

        Ok(hooks.live.status(found))
    }

    /// Notes that the session `id`, started outside Beep, was closed or seen
    /// to exit: it reads as stopped at once, not as active a moment ago
    /// from its last write.
    pub(crate) fn note_closed(&self, id: &SessionId) {
        let mut closed = lock(&self.0.closed);
        closed.retain(|_, at| at.elapsed() < CLOSED_FOR);
        closed.insert(id.to_owned(), Instant::now());
    }

    /// A session closed lately reads as stopped, not as active a moment ago
    /// from its last write.
    pub(crate) fn settle_closed(&self, sessions: &mut [ExternalSession]) {
        let mut closed = lock(&self.0.closed);

        if closed.is_empty() {
            return;
        }

        closed.retain(|_, at| at.elapsed() < CLOSED_FOR);

        for found in sessions {
            if found.pid.is_none()
                && found.presence == Presence::Recent
                && closed.contains_key(&found.id)
            {
                found.presence = Presence::Stopped;
            }
        }
    }

    /// What `found`, a session in `agent`'s logs, is doing, once a recent
    /// write alone is checked: Codex keeps a session's rollout open while it
    /// runs, and a session Beep ran here ended with Beep.
    pub(crate) async fn presence_of(&self, agent: Agent, found: &ExternalSession) -> Presence {
        if let Some(registered) = self
            .presence()
            .snapshot()
            .into_iter()
            .find(|native| native.agent == agent.as_ref() && native.id == found.id)
        {
            return registered.presence;
        }

        let held = self.held(agent, std::slice::from_ref(found)).await;
        settle(
            found.presence,
            found.pid,
            held.get(&found.id).copied().unwrap_or(Holding::Unknown),
        )
    }

    /// What holds the file of each of `found` that no process was seen
    /// running, by session id, all looked up at once.
    pub(crate) async fn held(
        &self,
        agent: Agent,
        found: &[ExternalSession],
    ) -> HashMap<SessionId, Holding> {
        let ids: Vec<SessionId> = found
            .iter()
            .filter(|found| found.pid.is_none())
            .map(|found| found.id.clone())
            .collect();
        // Claude Code does not keep its transcript open, but its processes
        // were looked at already and none runs these sessions.
        if agent == Agent::Claude {
            return ids.into_iter().map(|id| (id, Holding::Closed)).collect();
        }

        if ids.is_empty() {
            return HashMap::new();
        }

        let stores = self.stores().clone();
        let files = tokio::task::spawn_blocking(move || {
            ids.into_iter()
                .filter_map(|id| Some((stores.file(Some(agent), &id)?, id)))
                .collect::<HashMap<std::path::PathBuf, SessionId>>()
        })
        .await
        .unwrap_or_else(|e| {
            log::warn!("could not find the sessions' files: {e}");
            HashMap::new()
        });
        let open = self
            .0
            .holders
            .held_open(files.keys().cloned().collect())
            .await;
        files
            .into_iter()
            .map(|(file, id)| {
                let holding = open.get(&file).copied().unwrap_or(Holding::Unknown);
                (id, holding)
            })
            .collect()
    }
}
