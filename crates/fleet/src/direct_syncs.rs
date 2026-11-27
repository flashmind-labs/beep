//! What nodes that push no sync status last said of their sync (`DirectSyncs`).

use crate::changed::Changed;
use beep_mesh::PeerClient;
use beep_proto::node::sync::{PROTOCOL, ProjectStatus, Reply, Request};
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    sync::mpsc::UnboundedSender,
    time::{Instant, timeout},
};

/// How long a direct sync status call waits on one node.
const SYNC_CALL_LIMIT: Duration = Duration::from_secs(2);
/// How long the first direct read of a node waits for its answer, when nothing is kept yet.
pub(crate) const SYNC_FIRST_WAIT: Duration = Duration::from_millis(750);
/// How long a node's last direct answer is used before a background call asks again.
pub(crate) const SYNC_FRESH: Duration = Duration::from_secs(5);

/// Reads every project `client`'s node syncs, directly: for a node that does not offer the
/// `SyncList` topic and for a machine whose subscription is not live.
pub(crate) async fn fetch_node_statuses(client: &PeerClient) -> Option<Vec<ProjectStatus>> {
    let reply: Reply = client
        .call_within(SYNC_CALL_LIMIT, PROTOCOL, &Request::Status {})
        .await
        .ok()?;

    match reply {
        Reply::Status { projects } => Some(projects),
        _ => None,
    }
}

/// The last direct answer of one node that does not push its sync.
struct Direct {
    /// When it was asked; `None` for a first wait that gave up, so the next read asks again.
    at: Option<Instant>,
    projects: Option<Vec<ProjectStatus>>,
    refreshing: bool,
}

/// What nodes that do not offer the `SyncList` topic last said of their sync. A read answers
/// from it at once and a background call refreshes it. A node that pushes is never put here.
pub(crate) struct DirectSyncs {
    held: Mutex<HashMap<EndpointId, Direct>>,
    /// The machines that answered `SyncList` once: their node pushes, so a late snapshot is
    /// waited for and never replaced by a direct call.
    pushing: Mutex<HashSet<EndpointId>>,
    changes: UnboundedSender<Changed>,
}

impl DirectSyncs {
    pub(crate) fn new(changes: UnboundedSender<Changed>) -> Self {
        Self {
            held: Mutex::default(),
            pushing: Mutex::default(),
            changes,
        }
    }

    pub(crate) fn pushes(&self, machine: EndpointId) -> bool {
        self.pushing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&machine)
    }

    /// `machine` answered `SyncList`: nothing direct is kept or asked for it again.
    pub(crate) fn pushed(&self, machine: EndpointId) {
        self.pushing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(machine);
        self.forget(machine);
    }

    /// `machine`'s node no longer offers `SyncList`: it is read directly again.
    pub(crate) fn unpushed(&self, machine: EndpointId) {
        self.pushing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&machine);
    }

    pub(crate) fn forget(&self, machine: EndpointId) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&machine);
    }

    /// What `machine` last said. With nothing kept, asks and waits a little; a kept answer older
    /// than `SYNC_FRESH` is used as it is while a background call asks again.
    pub(crate) async fn read(
        self: &Arc<Self>,
        machine: EndpointId,
        client: Option<PeerClient>,
    ) -> Option<Vec<ProjectStatus>> {
        let kept = {
            let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            held.get_mut(&machine).map(|direct| {
                let stale = direct.at.is_none_or(|at| at.elapsed() >= SYNC_FRESH);
                let refresh = stale && !direct.refreshing && client.is_some();
                direct.refreshing |= refresh;
                (direct.projects.clone(), refresh)
            })
        };

        let Some((projects, refresh)) = kept else {
            let said = timeout(SYNC_FIRST_WAIT, fetch_node_statuses(&client?)).await;
            let at = said.is_ok().then(Instant::now);
            let projects = said.ok().flatten();
            self.held
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(
                    machine,
                    Direct {
                        at,
                        projects: projects.clone(),
                        refreshing: false,
                    },
                );

            return projects;
        };

        if refresh && let Some(client) = client {
            let this = self.clone();
            tokio::spawn(async move {
                let fetched = fetch_node_statuses(&client).await;
                this.refreshed(machine, fetched);
            });
        }

        projects
    }

    /// Keeps a background answer, and a failed call keeps what was said before.
    pub(crate) fn refreshed(&self, machine: EndpointId, fetched: Option<Vec<ProjectStatus>>) {
        let changed = {
            let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(direct) = held.get_mut(&machine) else {
                return;
            };
            direct.refreshing = false;
            direct.at = Some(Instant::now());

            match fetched {
                Some(projects) if direct.projects.as_ref() != Some(&projects) => {
                    direct.projects = Some(projects);
                    true
                }
                _ => false,
            }
        };

        if changed {
            let _ = self.changes.send(Changed::Sync);
        }
    }
}
