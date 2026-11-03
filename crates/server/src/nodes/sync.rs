//! Project sync (`sync/1`): the server tells each of a user's nodes which
//! projects to keep in step, in which folder and with which other nodes.
//! The nodes keep that list and sync node to node, so the server going
//! away stops nothing; it only reads back how each one stands.

use super::{Nodes, Online};
use crate::{
    AsyncMutex,
    protocol::node::sync::{
        CONTROLS, Keep, MAX_SKIPPED, PROTOCOL, Project, ProjectStatus, RESCAN, Refused, Reply,
        Request, Skipped,
    },
    store::SyncRow,
    user::User,
};
use anyhow::{Context, Result, bail};
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::{HashMap, HashSet},
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
/// How long a node may take to scan a project when asked.
const RESCAN_LIMIT: Duration = Duration::from_secs(130);
/// How long a node may take to settle a conflict.
const RESOLVE_LIMIT: Duration = Duration::from_secs(30);

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

    /// Whether this node pauses projects and settles conflicts.
    pub fn sync_controls(&self) -> bool {
        self.syncs() && self.offers(CONTROLS)
    }

    /// Whether this node scans a project when asked.
    pub fn sync_rescans(&self) -> bool {
        self.syncs() && self.offers(RESCAN)
    }

    /// Has this node scan `project` now and bring it in line; gives how
    /// it stands after.
    pub async fn sync_rescan(&self, project: &str) -> Result<ProjectStatus> {
        let request = Request::Rescan {
            project: project.to_owned(),
        };
        match self.sync_call(&request, RESCAN_LIMIT).await? {
            Reply::Rescanned { status } => Ok(status),
            other => bail!("{} answered {other:?}", self.name),
        }
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

    /// Settles the conflict copy at `path` of `project` on this node.
    pub async fn sync_resolve(&self, project: &str, path: &str, keep: Keep) -> Result<()> {
        if !self.sync_controls() {
            bail!("{}'s beep-node is too old to settle conflicts", self.name);
        }
        let request = Request::Resolve {
            project: project.to_owned(),
            path: path.to_owned(),
            keep,
        };
        match self.sync_call(&request, RESOLVE_LIMIT).await? {
            Reply::Resolved {} => Ok(()),
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

/// The rows of machines whose keys are still linked.
fn linked_rows(rows: Vec<SyncRow>, linked: &HashSet<String>) -> Vec<SyncRow> {
    rows.into_iter()
        .filter(|row| linked.contains(&row.endpoint))
        .collect()
}

/// What `projects_for` needs to know of the user and the node.
struct ForNode<'a> {
    node: EndpointId,
    /// The projects whose sync is paused.
    paused: &'a HashSet<String>,
    /// The node pauses a project itself; an older one is left without it.
    controls: bool,
}

/// The projects `node` syncs, each with the other nodes that sync it. A
/// paused project goes as paused to a node that pauses, and is left out
/// for one that does not, which stops it.
fn projects_for(rows: &[SyncRow], node: &ForNode, state: &SyncState) -> Vec<Project> {
    let id = node.node.to_string();
    rows.iter()
        .filter(|row| row.endpoint == id)
        .filter(|row| node.controls || !node.paused.contains(&row.project))
        .map(|row| Project {
            name: row.project.clone(),
            folder: row.folder.clone(),
            peers: rows
                .iter()
                .filter(|peer| peer.project == row.project && peer.endpoint != id)
                .filter_map(|peer| peer.endpoint.parse().ok())
                .map(|peer| state.addr(peer))
                .collect(),
            paused: node.paused.contains(&row.project),
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
    // Only machines still linked sync, and only with each other.
    let linked: HashSet<String> = user
        .account_keys()
        .await?
        .into_iter()
        .map(|key| key.endpoint_id)
        .collect();
    let rows = linked_rows(user.project_syncs().await?, &linked);
    let paused: HashSet<String> = user.paused_syncs().await?.into_iter().collect();
    let mut online: Vec<Online> = nodes
        .of_user(user.id())
        .into_iter()
        .filter(Online::syncs)
        .collect();
    online.sort_by_key(|node| Some(node.id()) != first);
    let mut refusals = Vec::new();
    for node in online {
        let id = node.id();
        let for_node = ForNode {
            node: id,
            paused: &paused,
            controls: node.sync_controls(),
        };
        let projects = projects_for(&rows, &for_node, &nodes.sync);
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

    /// A node that pauses, with nothing paused.
    fn plain(node: EndpointId, none: &HashSet<String>) -> ForNode<'_> {
        ForNode {
            node,
            paused: none,
            controls: true,
        }
    }

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
        let none = HashSet::new();
        let projects = projects_for(&rows, &plain(a, &none), &state);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "game");
        assert_eq!(projects[0].folder, "/srv/game");
        assert_eq!(projects[0].peers, [known]);

        assert!(!projects[0].paused);

        let names: Vec<_> = projects_for(&rows, &plain(b, &none), &state)
            .into_iter()
            .map(|project| (project.name, project.peers.len()))
            .collect();
        assert_eq!(names, [("game".to_owned(), 1), ("site".to_owned(), 1)]);
        let stranger = SecretKey::generate().public();
        assert!(projects_for(&rows, &plain(stranger, &none), &state).is_empty());
    }

    #[test]
    fn a_paused_project_goes_paused_or_is_left_out_for_an_older_node() {
        let a = SecretKey::generate().public();
        let rows = ["game", "site"].map(|project| SyncRow {
            project: project.into(),
            endpoint: a.to_string(),
            folder: format!("/srv/{project}"),
        });
        let paused: HashSet<String> = ["game".to_owned()].into();
        let state = SyncState::default();
        let newer = ForNode {
            node: a,
            paused: &paused,
            controls: true,
        };
        let got: Vec<_> = projects_for(&rows, &newer, &state)
            .into_iter()
            .map(|project| (project.name, project.paused))
            .collect();
        assert_eq!(got, [("game".to_owned(), true), ("site".to_owned(), false)]);
        let older = ForNode {
            controls: false,
            ..newer
        };
        let got: Vec<_> = projects_for(&rows, &older, &state)
            .into_iter()
            .map(|project| project.name)
            .collect();
        assert_eq!(got, ["site"]);
    }

    #[test]
    fn a_machine_no_longer_linked_is_nobody_s_peer() {
        let [a, b] = [(); 2].map(|()| SecretKey::generate().public());
        let rows = [a, b]
            .map(|node| SyncRow {
                project: "game".into(),
                endpoint: node.to_string(),
                folder: "/srv/game".into(),
            })
            .to_vec();
        let linked: HashSet<String> = [a.to_string()].into();
        let rows = linked_rows(rows, &linked);
        let none = HashSet::new();
        let projects = projects_for(&rows, &plain(a, &none), &SyncState::default());
        assert_eq!(projects.len(), 1);
        assert!(projects[0].peers.is_empty());
        assert!(projects_for(&rows, &plain(b, &none), &SyncState::default()).is_empty());
    }
}
