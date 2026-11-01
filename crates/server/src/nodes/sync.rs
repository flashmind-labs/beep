//! Project sync (`sync/1`): the server tells each of a user's nodes which
//! projects to keep in step, in which folder and with which other nodes.
//! The nodes keep that list and sync node to node, so the server going
//! away stops nothing; it only reads back how each one stands.

use super::{Nodes, Online};
use crate::{
    AsyncMutex,
    protocol::node::sync::{
        MAX_SKIPPED, PROTOCOL, Project, ProjectStatus, Refused, Reply, Request, Skipped,
    },
    store::SyncRow,
    user::User,
};
use anyhow::{Context, Result, bail};
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::time::timeout;

/// How long a node may take to start or stop its projects.
const CONFIGURE_LIMIT: Duration = Duration::from_secs(30);
/// How long a node may take to say how its projects stand, or to list
/// what one leaves out.
const STATUS_LIMIT: Duration = Duration::from_secs(5);
const SKIPPED_LIMIT: Duration = Duration::from_secs(30);

/// What the server keeps between pushes.
#[derive(Default)]
pub struct SyncState {
    /// How each node said it is reached, for its peers.
    addrs: Mutex<HashMap<EndpointId, EndpointAddr>>,
    /// One push per user at a time, so an older list never lands last.
    pushing: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl SyncState {
    fn addrs(&self) -> MutexGuard<'_, HashMap<EndpointId, EndpointAddr>> {
        self.addrs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn pushing(&self, user: &str) -> Arc<AsyncMutex<()>> {
        self.pushing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(user.to_owned())
            .or_default()
            .clone()
    }

    /// How `node` is reached: what it last said, or its key alone, which
    /// discovery resolves.
    fn addr(&self, node: EndpointId) -> EndpointAddr {
        self.addrs()
            .get(&node)
            .cloned()
            .unwrap_or_else(|| node.into())
    }
}

/// A project a node would not sync.
#[derive(Debug, Clone)]
pub struct Refusal {
    pub node: EndpointId,
    pub refused: Refused,
}

impl Online {
    /// Whether this node can sync projects.
    pub fn syncs(&self) -> bool {
        self.offers(PROTOCOL)
    }

    async fn sync_call(&self, request: &Request, limit: Duration) -> Result<Reply> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(request).await?;
        let reply = timeout(limit, rx.recv::<Reply>())
            .await
            .with_context(|| format!("{} did not answer in time", self.name))??
            .with_context(|| format!("{} closed the stream", self.name))?;
        if let Reply::Error { message } = reply {
            bail!("{}: {message}", self.name);
        }
        Ok(reply)
    }

    /// How each project this node syncs stands.
    pub async fn sync_status(&self) -> Result<Vec<ProjectStatus>> {
        match self.sync_call(&Request::Status {}, STATUS_LIMIT).await? {
            Reply::Status { projects } => Ok(projects),
            other => bail!("{} answered {other:?}", self.name),
        }
    }

    /// What `project`'s sync leaves out on this node.
    pub async fn sync_skipped(&self, project: &str) -> Result<Skipped> {
        let request = Request::Skipped {
            project: project.to_owned(),
            limit: MAX_SKIPPED,
        };
        match self.sync_call(&request, SKIPPED_LIMIT).await? {
            Reply::Skipped { skipped } => Ok(skipped),
            other => bail!("{} answered {other:?}", self.name),
        }
    }

    /// Sets the projects this node syncs; gives how it is reached and
    /// what it refused.
    async fn configure_sync(&self, projects: Vec<Project>) -> Result<Configured> {
        match self
            .sync_call(&Request::Configure { projects }, CONFIGURE_LIMIT)
            .await?
        {
            Reply::Configured { addr, refused } => Ok(Configured { addr, refused }),
            other => bail!("{} answered {other:?}", self.name),
        }
    }

    fn id(&self) -> EndpointId {
        self.connection.remote_id()
    }
}

struct Configured {
    addr: EndpointAddr,
    refused: Vec<Refused>,
}

/// The projects `node` syncs, each with the other nodes that sync it.
fn projects_for(rows: &[SyncRow], node: EndpointId, state: &SyncState) -> Vec<Project> {
    let id = node.to_string();
    rows.iter()
        .filter(|row| row.endpoint == id)
        .map(|row| Project {
            name: row.project.clone(),
            folder: row.folder.clone(),
            peers: rows
                .iter()
                .filter(|peer| peer.project == row.project && peer.endpoint != id)
                .filter_map(|peer| peer.endpoint.parse().ok())
                .map(|peer| state.addr(peer))
                .collect(),
        })
        .collect()
}

/// Tells each of `user`'s online nodes that can sync which projects to
/// sync, where, and with whom. `first` goes first, so the others get how
/// it is reached now. A node that fails is skipped: it gets its list when
/// it connects again.
pub async fn push_sync(
    nodes: &Nodes,
    user: &User,
    first: Option<EndpointId>,
) -> Result<Vec<Refusal>> {
    let pushing = nodes.sync.pushing(user.id());
    let _one = pushing.lock().await;
    let rows = user.project_syncs().await?;
    let mut online: Vec<Online> = nodes
        .of_user(user.id())
        .into_iter()
        .filter(Online::syncs)
        .collect();
    online.sort_by_key(|node| Some(node.id()) != first);
    let mut refusals = Vec::new();
    for node in online {
        let id = node.id();
        let projects = projects_for(&rows, id, &nodes.sync);
        match node.configure_sync(projects).await {
            Ok(configured) => {
                nodes.sync.addrs().insert(id, configured.addr);
                refusals.extend(
                    configured
                        .refused
                        .into_iter()
                        .map(|refused| Refusal { node: id, refused }),
                );
            }
            Err(e) => log::warn!("sync list not sent; node={id} error={e:#}"),
        }
    }
    for refusal in &refusals {
        log::info!(
            "node refused to sync; node={} project={:?} reason={:?}",
            refusal.node,
            refusal.refused.project,
            refusal.refused.message
        );
    }
    Ok(refusals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn a_node_gets_its_projects_with_the_other_nodes_as_peers() {
        let [a, b, c] = [(); 3].map(|()| SecretKey::generate().public());
        let row = |project: &str, node: EndpointId| SyncRow {
            project: project.into(),
            endpoint: node.to_string(),
            folder: format!("/srv/{project}"),
        };
        let rows = [
            row("game", a),
            row("game", b),
            row("site", b),
            row("site", c),
        ];
        let state = SyncState::default();
        let known: EndpointAddr = b.into();
        state.addrs().insert(b, known.clone());
        let projects = projects_for(&rows, a, &state);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "game");
        assert_eq!(projects[0].folder, "/srv/game");
        assert_eq!(projects[0].peers, [known]);

        let names: Vec<_> = projects_for(&rows, b, &state)
            .into_iter()
            .map(|project| (project.name, project.peers.len()))
            .collect();
        assert_eq!(names, [("game".to_owned(), 1), ("site".to_owned(), 1)]);
        let none = SecretKey::generate().public();
        assert!(projects_for(&rows, none, &state).is_empty());
    }
}
