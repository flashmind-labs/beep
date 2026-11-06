//! Nodes: machines running `beep-node`, each holding one connection open on `NODE_ALPN`, or
//! `NODE_ALPN_NAMED` for a node from before it.

mod describe;
mod link;
mod serve;
pub use describe::describe;
pub(crate) use describe::{MACHINES_HEADER, machines};
pub use link::{NewToken, TokenRefusal, create_token, link_machine};
pub use serve::{NodeHandler, accept_nodes};

pub mod attachments;
pub mod workspace;

use crate::app::App;
use iroh::{
    EndpointId,
    endpoint::{Connection, VarInt},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak},
    time::Duration,
};
use tokio::{
    sync::Notify,
    time::{Instant, timeout_at},
};

mod agent;
mod alerts;
mod deliver;
mod desktop;
mod files;
mod follow;
mod harness;
mod images;
mod logs;
mod retry;
mod search;
mod shell;
mod stats;
mod sync;
mod tasks;
mod terminals;
mod tool_update;
pub mod transfer;
mod update;
mod usage;

pub(crate) use crate::protocol::agent_name;
pub use crate::protocol::node::{
    NODE_ALPN, NODE_ALPN_NAMED, desktop::PROTOCOL as DESKTOP, files::PROTOCOL as FILES,
    shell::PROTOCOL as SHELL, sync::PROTOCOL as SYNC, transfer::PROTOCOL as TRANSFER,
};
pub use agent::{AgentLink, Mark, StartOn, Watched};
pub(crate) use agent::{News, NodeFile, NodeImage, NodeWork, SessionRefused, WorkRead, WorkWrite};
pub use deliver::{Ask, Deliveries, Post};
pub use follow::{Follower, Follows};
pub(crate) use retry::{NODE_BACK, Repeat};
pub use shell::{Exited, Ran};
pub use stats::Sampled;
pub use sync::{Refusal, SyncState, push_sync};

const CLOSE_UNAVAILABLE: u32 = 503;
/// The most calls one node's agents have running through the server.
const MAX_TOOL_CALLS: usize = 16;

/// A node with its connection open.
#[derive(Clone)]
pub struct Online {
    pub user: String,
    pub name: String,
    pub offers: Vec<String>,
    /// The user's files and code folder, when this node is their environment.
    pub workspace: Option<String>,
    /// It makes the branch or worktree a session's start asks for.
    pub checkouts: bool,
    /// It also makes named and existing-branch worktrees; older nodes cannot read such a start.
    pub branch_checkouts: bool,
    pub connection: Connection,
    /// Held weakly: the app owns the nodes.
    app: Weak<App>,
    shell: Arc<shell::ShellSlot>,
    agents: Arc<agent::AgentLinks>,
    desktop: Arc<desktop::DesktopSlot>,
    stats: Arc<stats::StatsSlot>,
    /// Woken when the user's credentials change, for a node that takes them.
    credentials: Option<Arc<Notify>>,
    /// Woken when this machine's harness settings change, for a node that takes them.
    agent_settings: Option<Arc<Notify>>,
    /// Woken to have the node read its agents' plans again.
    usage_refresh: Arc<Notify>,
}

/// The nodes connected now, by key.
#[derive(Default)]
pub struct Nodes {
    online: Mutex<HashMap<EndpointId, Online>>,
    /// The server's own key, which link offers are signed for.
    server: OnceLock<EndpointId>,
    /// Sessions started outside Beep that chats follow.
    pub follows: Follows,
    /// What agent sessions tell their chats.
    pub deliveries: Deliveries,
    /// What project sync keeps between pushes.
    pub sync: SyncState,
    /// When each node that is offline now last dropped its connection.
    left: Mutex<HashMap<EndpointId, Instant>>,
    /// Woken when a node connects or drops, for calls waiting on one.
    changed: Notify,
    /// The newest `gh` and `teacli` releases, for `ListToolUpdates`.
    pub releases: beep_mesh::machine::releases::ToolReleases,
}

/// How long after a node drops a call still waits for it to come back:
/// long enough for a restart, such as after `beep-node update`.
const LEFT_LATELY: Duration = Duration::from_secs(60);
/// How long a call waits in all for a node to come back.
pub(crate) const RECONNECT_WAIT: Duration = Duration::from_secs(20);
/// The first and longest pause between looks at a node coming back, on top
/// of the wake-up a new connection gives.
const FIRST_LOOK: Duration = Duration::from_millis(250);
const LAST_LOOK: Duration = Duration::from_secs(4);

impl Nodes {
    fn lock(&self) -> MutexGuard<'_, HashMap<EndpointId, Online>> {
        self.online.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The server's key, once it listens for nodes.
    pub fn server(&self) -> Option<EndpointId> {
        self.server.get().copied()
    }

    /// The user's online node that keeps a workspace and offers shell and files.
    pub fn workspace(&self, user: &str) -> Option<Online> {
        self.lock()
            .values()
            .find(|online| {
                online.user == user
                    && online.workspace.is_some()
                    && online.offers(SHELL)
                    && online.offers(FILES)
            })
            .cloned()
    }

    /// The user's nodes online now.
    pub fn of_user(&self, user: &str) -> Vec<Online> {
        self.lock()
            .values()
            .filter(|online| online.user == user)
            .cloned()
            .collect()
    }

    /// The connected node with the key `node`, as the store keeps it.
    pub fn online(&self, node: &str) -> Option<Online> {
        let id: EndpointId = node.parse().ok()?;
        self.lock().get(&id).cloned()
    }

    /// Tells the user's nodes that take credentials to fetch them again.
    pub fn credentials_changed(&self, user: &str) {
        for online in self.lock().values().filter(|online| online.user == user) {
            if let Some(changed) = &online.credentials {
                changed.notify_one();
            }
        }
    }

    /// Tells the user's node `machine_id` to fetch its harness settings again.
    pub fn agent_settings_changed(&self, user: &str, machine_id: &str) {
        let Ok(id) = machine_id.parse::<EndpointId>() else {
            return;
        };
        if let Some(online) = self.lock().get(&id).filter(|online| online.user == user)
            && let Some(changed) = &online.agent_settings
        {
            changed.notify_one();
        }
    }

    /// Tells every online node of the user to fetch its harness settings again.
    pub fn agent_settings_changed_everywhere(&self, user: &str) {
        for online in self.lock().values().filter(|online| online.user == user) {
            if let Some(changed) = &online.agent_settings {
                changed.notify_one();
            }
        }
    }

    fn insert(&self, node: EndpointId, online: Online) {
        // A node that reconnects replaces its old connection.
        let old = self.lock().insert(node, online);
        if let Some(old) = old {
            old.connection.close(VarInt::from_u32(0), b"replaced");
        }
        self.left().remove(&node);
        self.changed.notify_waiters();
    }

    fn left(&self) -> MutexGuard<'_, HashMap<EndpointId, Instant>> {
        self.left.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `node` is offline now but dropped its connection within
    /// `LEFT_LATELY`, as a node that restarts does.
    fn left_lately(&self, node: &EndpointId) -> bool {
        self.left()
            .get(node)
            .is_some_and(|at| at.elapsed() < LEFT_LATELY)
    }

    /// The node `node` (its key as the store keeps it) once it is back
    /// online, when it dropped its connection lately; waits up to
    /// `RECONNECT_WAIT`. `None` for a node that has been offline for a
    /// while, or did not come back in time.
    pub(crate) async fn back_online(&self, node: &str) -> Option<Online> {
        let id: EndpointId = node.parse().ok()?;
        if let Some(online) = self.online(node) {
            return Some(online);
        }
        if !self.left_lately(&id) {
            return None;
        }
        self.reconnected(&id, None, Instant::now() + RECONNECT_WAIT)
            .await
    }

    /// `node`'s open connection other than `than` (a `stable_id`), waiting
    /// for one until `until`. Woken when a node connects or drops, and
    /// looks again after a pause that grows from `FIRST_LOOK` to
    /// `LAST_LOOK`, since a connection can close without either.
    pub(crate) async fn reconnected(
        &self,
        node: &EndpointId,
        than: Option<usize>,
        until: Instant,
    ) -> Option<Online> {
        let mut pause = FIRST_LOOK;
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            // Registered before the look, so a connection that comes in
            // between still wakes it.
            changed.as_mut().enable();
            let current = self.lock().get(node).cloned();
            if let Some(online) = current.filter(|online| {
                online.connection.close_reason().is_none()
                    && than != Some(online.connection.stable_id())
            }) {
                return Some(online);
            }
            let now = Instant::now();
            if now >= until {
                return None;
            }
            let look = until.min(now + pause);
            // Either wake-up leads to another look.
            let _ = timeout_at(look, changed).await;
            pause = (pause * 2).min(LAST_LOOK);
        }
    }

    /// Drops `node`'s connection with close code `reason`.
    pub fn forget(&self, node: &EndpointId, reason: u32) {
        let removed = self.lock().remove(node);
        if let Some(online) = removed {
            online
                .connection
                .close(VarInt::from_u32(reason), b"signed out");
        }
        self.changed.notify_waiters();
    }

    /// Forgets `node` if `connection` is still the one it holds.
    fn remove(&self, node: &EndpointId, connection: &Connection) {
        let removed = {
            let mut nodes = self.lock();
            let current = nodes
                .get(node)
                .is_some_and(|online| online.connection.stable_id() == connection.stable_id());
            current && nodes.remove(node).is_some()
        };
        if removed {
            let mut left = self.left();
            left.retain(|_, at| at.elapsed() < LEFT_LATELY);
            left.insert(*node, Instant::now());
        }
        self.changed.notify_waiters();
    }
}
