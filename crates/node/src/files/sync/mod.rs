//! `sync/1` (see `beep_proto::node::sync`): keeps project folders in step
//! with the same project on the user's other machines. `Syncs::configure`
//! takes the server's project list.

use crate::{
    AsyncMutex,
    files::Folders,
    files::syncthing::{Managed, Syncthing},
    files::transfer::{
        grants::Access,
        place::{Journal, Room, open_target},
    },
    link::{Protocol, Session},
    lock,
};
use anyhow::{Context, Result, bail};
use beep_mesh::Mesh;
use beep_proto::node::{
    LinkRx, LinkTx,
    sync::{
        CONTROLS, Keep, PROTOCOL, Project, ProjectStatus, RESCAN, Refused, Reply, Request, Skipped,
        State,
    },
};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_blobs::{Hash, store::fs::FsStore};
use lists::Lists;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    task::spawn_blocking,
    time::{Instant, timeout},
};
use tokio_util::task::AbortOnDropHandle;

mod disk;
mod gitconfig;
mod gitdir;
mod lists;
mod map;
#[cfg(test)]
mod map_sim;
mod peer;
mod progress;
mod project;
mod root;
mod rules;
mod scan;
mod stignore;
mod watch;

pub(crate) use peer::{ALPN, PeerHandler};
use project::{Shared, Start};
use watch::{Key, Watcher};

/// How long a scan `Request::Rescan` asks for may take.
const RESCAN_LIMIT: Duration = Duration::from_secs(120);
/// How long a project that stops may take to save its state.
const STOP_LIMIT: Duration = Duration::from_secs(10);
/// The projects the server last named (which, where, with which peers),
/// kept so sync goes on while the server is down.
const CONFIG: &str = "projects.json";

/// What sync shares with transfers.
#[derive(Clone)]
pub(crate) struct Parts {
    pub(crate) endpoint: Endpoint,
    pub(crate) store: FsStore,
    pub(crate) store_dir: PathBuf,
    pub(crate) journal: Journal,
    pub(crate) room: Arc<Room>,
}

/// The projects as the server last set them.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    projects: Vec<Project>,
}

struct Running {
    project: Project,
    shared: Arc<Shared>,
    task: AbortOnDropHandle<()>,
}

/// A project the server set that Syncthing already keeps in step: kept on
/// the list, not synced, until Syncthing lets go of its folder.
struct Held {
    project: Project,
    managed: Managed,
}

impl Held {
    fn status(&self) -> ProjectStatus {
        ProjectStatus {
            name: self.project.name.clone(),
            folder: self.project.folder.clone(),
            state: State::Failed,
            error: Some(self.managed.reason()),
            ..ProjectStatus::default()
        }
    }
}

/// Where a list of projects comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Saved on disk: the server set each of them before.
    Saved,
    /// The server, which may be turning one on now.
    Server,
}

struct Inner {
    parts: Parts,
    folders: Folders,
    /// Where the projects' state is kept.
    dir: PathBuf,
    watcher: Watcher,
    projects: Mutex<BTreeMap<String, Running>>,
    held: Mutex<BTreeMap<String, Held>>,
    syncthing: Syncthing,
    /// Checks the folders against Syncthing's every so often.
    checking: Mutex<Option<AbortOnDropHandle<()>>>,
    /// One configure at a time.
    configuring: AsyncMutex<()>,
    /// The pulls other nodes have open here.
    serving: Arc<AtomicUsize>,
    /// The mesh this node follows, once it is in one: it names the
    /// projects, and its device list says who may sync them.
    mesh: Mutex<Option<Mesh>>,
    /// What the mesh and the server each say of the projects.
    lists: Mutex<Lists>,
}

/// Stops `running` and waits for its task, which saves its state, a while.
async fn halt(running: Running) -> Arc<Shared> {
    running.shared.stop();
    if timeout(STOP_LIMIT, running.task).await.is_err() {
        log::warn!("{} did not stop in time", running.shared.name());
    }
    running.shared
}

/// Counts one pull served while it lives.
pub(super) struct Serving(Arc<AtomicUsize>);

impl Drop for Serving {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The node's synced projects.
#[derive(Clone)]
pub struct Syncs {
    inner: Arc<Inner>,
}

impl Inner {
    /// Whether `peer` was removed from the mesh the node is in. Asked on
    /// every connection and request, so a removed device is out at once,
    /// before the project's peer list catches up. A device that never
    /// joined is no concern of the mesh: a machine still linked through
    /// the server keeps the grants the server issued it.
    fn removed(&self, peer: &EndpointId) -> bool {
        lock(&self.mesh)
            .as_ref()
            .is_some_and(|mesh| mesh.is_removed(*peer))
    }
}

impl Access for Inner {
    fn allows(&self, peer: &EndpointId) -> bool {
        !self.removed(peer)
    }

    fn may_connect(&self, peer: &EndpointId) -> bool {
        self.allows(peer)
            && lock(&self.projects)
                .values()
                .any(|running| running.shared.has_peer(peer))
    }

    fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool {
        self.allows(peer)
            && lock(&self.projects)
                .values()
                .any(|running| running.shared.has_peer(peer) && running.shared.has(hash))
    }
}

/// Which state a project in `folder` keeps; another folder starts over.
fn key_of(name: &str, folder: &Path) -> Key {
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend_from_slice(folder.as_os_str().as_encoded_bytes());
    let hash = Hash::new(&bytes);
    let mut key = [0; 8];
    key.copy_from_slice(&hash.as_bytes()[..8]);
    u64::from_be_bytes(key)
}

impl Syncs {
    /// Starts the projects saved in `dir`, as the server last set them,
    /// holding those `syncthing` keeps in step.
    pub(crate) async fn start(
        parts: Parts,
        folders: Folders,
        dir: PathBuf,
        syncthing: Syncthing,
    ) -> Result<Self> {
        fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        let config = match fs::read(dir.join(CONFIG)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("starting with no synced projects: {CONFIG} is unreadable: {e}");
                Config::default()
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {CONFIG}")),
        };
        // Reading the watch budget walks `/proc`.
        let watcher = spawn_blocking(Watcher::new).await?;
        let syncs = Self {
            inner: Arc::new(Inner {
                parts,
                folders,
                dir,
                watcher,
                projects: Mutex::default(),
                held: Mutex::default(),
                syncthing,
                checking: Mutex::default(),
                configuring: AsyncMutex::new(()),
                serving: Arc::default(),
                mesh: Mutex::default(),
                lists: Mutex::default(),
            }),
        };
        // What the server set before keeps its peers while the mesh waits for a machine.
        lock(&syncs.inner.lists).set_server(&config.projects);
        for refused in syncs.configure_from(config.projects, Source::Saved).await {
            log::warn!("not syncing {}: {}", refused.project, refused.message);
        }
        let task = tokio::spawn(check_syncthing(
            Arc::downgrade(&syncs.inner),
            syncs.inner.syncthing.clone(),
        ));
        *lock(&syncs.inner.checking) = Some(AbortOnDropHandle::new(task));
        Ok(syncs)
    }

    /// What lets peers fetch the projects' files.
    pub(crate) fn access(&self) -> Arc<dyn Access> {
        self.inner.clone()
    }

    /// How other nodes reach this one.
    pub fn addr(&self) -> EndpointAddr {
        self.inner.parts.endpoint.addr()
    }

    /// Syncs exactly `projects`, and says which it refused. A project left
    /// out stops; its files stay and its state is forgotten. A folder
    /// Syncthing keeps in step is refused when it is new here, and held
    /// when the server set it before.
    pub async fn configure(&self, projects: Vec<Project>) -> Vec<Refused> {
        lock(&self.inner.lists).set_server(&projects);
        self.configure_from(projects, Source::Server).await
    }

    /// Follows `mesh` from now on: its device list says who may sync, and
    /// the server's project list is ignored.
    pub(crate) fn follow(&self, mesh: Mesh) {
        *lock(&self.inner.mesh) = Some(mesh);
    }

    /// Lets go of the mesh `follow` took.
    pub(crate) fn unfollow(&self) {
        *lock(&self.inner.mesh) = None;
        lock(&self.inner.lists).clear_mesh();
    }

    /// Whether the mesh owns the project list, so the server's is ignored.
    fn follows_mesh(&self) -> bool {
        lock(&self.inner.mesh)
            .as_ref()
            .is_some_and(|mesh| mesh.has_held("sync/"))
    }

    /// Syncs the projects the mesh names, `pending` being those that still name
    /// a machine that has not joined it: those keep the peers the server set.
    pub(crate) async fn configure_from_mesh(
        &self,
        projects: Vec<Project>,
        pending: BTreeSet<String>,
    ) -> Vec<Refused> {
        lock(&self.inner.lists).set_mesh(projects, pending);
        self.apply_lists().await
    }

    /// The server's list while the mesh owns it: only the peers of a project
    /// still waiting for a machine are taken. It says which of its projects
    /// the mesh does not name, which do not sync here.
    pub(crate) async fn configure_beside_mesh(&self, projects: Vec<Project>) -> Vec<Refused> {
        let ignored: Vec<Refused> = {
            let mut lists = lock(&self.inner.lists);
            lists.set_server(&projects);
            lists
                .not_in_mesh(&projects)
                .into_iter()
                .map(|name| Refused {
                    project: name.to_owned(),
                    message: "this machine takes its projects from its mesh".to_owned(),
                })
                .collect()
        };
        if !ignored.is_empty() {
            log::info!(
                "ignored the server's sync list for {} projects: the mesh sets them",
                ignored.len()
            );
        }
        // Before the mesh has been read there is nothing to put it beside.
        let mut refused = if lock(&self.inner.lists).has_mesh() {
            self.apply_lists().await
        } else {
            Vec::new()
        };
        refused.extend(ignored);
        refused
    }

    async fn apply_lists(&self) -> Vec<Refused> {
        let me = self.inner.parts.endpoint.id();
        let projects = lock(&self.inner.lists).effective(me, |peer| self.inner.removed(peer));
        self.configure_from(projects, Source::Server).await
    }

    async fn configure_from(&self, projects: Vec<Project>, from: Source) -> Vec<Refused> {
        let _one = self.inner.configuring.lock().await;
        let me = self.inner.parts.endpoint.id();
        let mut refused = Vec::new();
        let mut kept = Vec::new();
        let mut stopping = Vec::new();
        let mut replaced = Vec::new();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        {
            let mut running = lock(&self.inner.projects);
            let gone: Vec<String> = running
                .keys()
                .filter(|name| !names.contains(&name.as_str()))
                .cloned()
                .collect();
            for name in gone {
                stopping.extend(running.remove(&name));
            }
            lock(&self.inner.held).retain(|name, _| names.contains(&name.as_str()));
        }
        for mut project in projects {
            project.peers.retain(|peer| peer.id != me);
            let same = {
                let mut running = lock(&self.inner.projects);
                running
                    .get_mut(&project.name)
                    .filter(|running| running.project.folder == project.folder)
                    .map(|running| {
                        running.project = project.clone();
                        running.shared.clone()
                    })
            };
            if let Some(shared) = same {
                shared.set_peers(project.peers.clone());
                shared.set_paused(project.paused);
                kept.push(project);
                continue;
            }
            // Stopped before its replacement starts: the same folder named
            // another way has the same key, watches and state file.
            let old = lock(&self.inner.projects).remove(&project.name);
            if let Some(old) = old {
                replaced.push(halt(old).await);
            }
            let was_held = lock(&self.inner.held)
                .remove(&project.name)
                .is_some_and(|held| held.project.folder == project.folder);
            if let Some(managed) = self.inner.syncthing.check(Path::new(&project.folder)).await {
                if was_held || from == Source::Saved {
                    log::info!("not syncing {}: {}", project.name, managed.reason());
                    lock(&self.inner.held).insert(
                        project.name.clone(),
                        Held {
                            project: project.clone(),
                            managed,
                        },
                    );
                    kept.push(project);
                } else {
                    refused.push(Refused {
                        project: project.name.clone(),
                        message: managed.reason(),
                    });
                }
                continue;
            }
            match self.run(&project).await {
                Ok(running) => {
                    lock(&self.inner.projects).insert(project.name.clone(), running);
                    kept.push(project);
                }
                Err(e) => refused.push(Refused {
                    project: project.name.clone(),
                    message: format!("{e:#}"),
                }),
            }
        }
        for stopped in stopping {
            self.stop(stopped).await;
        }
        for old in replaced {
            self.forget(&old).await;
        }
        let config = Config { projects: kept };
        let saved = serde_json::to_vec_pretty(&config)
            .context("encoding the projects")
            .and_then(|bytes| crate::link::note::write(&self.inner.dir.join(CONFIG), &bytes));
        if let Err(e) = saved {
            log::warn!("the synced projects are not saved: {e:#}");
        }
        refused
    }

    async fn run(&self, project: &Project) -> Result<Running> {
        let (folders, folder) = (self.inner.folders.clone(), project.folder.clone());
        let root = spawn_blocking(move || open_target(&folders, &folder))
            .await?
            .with_context(|| format!("the folder {} cannot be synced", project.folder))?;
        let key = key_of(&project.name, root.path());
        let start = Start {
            name: project.name.clone(),
            key,
            root: Arc::new(root),
            peers: project.peers.clone(),
            file: self.inner.dir.join(format!("{key:016x}.state")),
            paused: project.paused,
        };
        let shared = spawn_blocking(move || Shared::load(start)).await??;
        let task = tokio::spawn(project::run(
            shared.clone(),
            self.inner.parts.clone(),
            self.inner.watcher.clone(),
        ));
        Ok(Running {
            project: project.clone(),
            shared,
            task: AbortOnDropHandle::new(task),
        })
    }

    async fn stop(&self, running: Running) {
        let shared = halt(running).await;
        self.forget(&shared).await;
    }

    /// Forgets the state of the stopped project `shared`, unless its folder
    /// runs again under the same key.
    async fn forget(&self, shared: &Shared) {
        let name = shared.name();
        let key = shared.key();
        // The same folder may have started again under the same key.
        if lock(&self.inner.projects)
            .values()
            .any(|other| other.shared.key() == key)
        {
            return;
        }
        // A project turned off forgets its folder: started again, it scans
        // from nothing, so a folder emptied meanwhile deletes nothing.
        let file = shared.file().to_owned();
        let removed = spawn_blocking(move || match fs::remove_file(&file) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })
        .await;
        match removed {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("the state of {name} stays: {e}"),
            Err(e) => log::warn!("the state of {name} stays: {e}"),
        }
        if let Err(e) = self
            .inner
            .parts
            .store
            .tags()
            .delete(project::tag_name(key))
            .await
        {
            log::warn!("the files of {name} stay in the store: {e}");
        }
    }

    /// Checks the synced folders against Syncthing's now: a project whose
    /// folder Syncthing keeps in step stops, as one turned off does, which
    /// deletes nothing anywhere, and is held; a held one whose folder
    /// Syncthing let go of or paused starts again, from a fresh scan.
    pub async fn check_syncthing(&self) {
        let _one = self.inner.configuring.lock().await;
        let running: Vec<Project> = lock(&self.inner.projects)
            .values()
            .map(|running| running.project.clone())
            .collect();
        for project in running {
            let Some(managed) = self.inner.syncthing.check(Path::new(&project.folder)).await else {
                continue;
            };
            log::info!("stopped syncing {}: {}", project.name, managed.reason());
            let stopped = lock(&self.inner.projects).remove(&project.name);
            if let Some(stopped) = stopped {
                self.stop(stopped).await;
            }
            lock(&self.inner.held).insert(project.name.clone(), Held { project, managed });
        }
        let held: Vec<Project> = lock(&self.inner.held)
            .values()
            .map(|held| held.project.clone())
            .collect();
        for project in held {
            if self
                .inner
                .syncthing
                .check(Path::new(&project.folder))
                .await
                .is_some()
            {
                continue;
            }
            match self.run(&project).await {
                Ok(running) => {
                    log::info!(
                        "syncing {}: Syncthing let go of it or paused it",
                        project.name
                    );
                    lock(&self.inner.held).remove(&project.name);
                    lock(&self.inner.projects).insert(project.name.clone(), running);
                }
                Err(e) => log::warn!("{} does not sync yet: {e:#}", project.name),
            }
        }
    }

    /// Stops every project, saving its state.
    pub(crate) async fn shutdown(&self) {
        let checking = lock(&self.inner.checking).take();
        drop(checking);
        let _one = self.inner.configuring.lock().await;
        let running = std::mem::take(&mut *lock(&self.inner.projects));
        for running in running.into_values() {
            halt(running).await;
        }
    }

    /// How each project stands.
    pub fn status(&self) -> Vec<ProjectStatus> {
        let mut out: Vec<ProjectStatus> = lock(&self.inner.projects)
            .values()
            .map(|running| running.shared.status())
            .collect();
        out.extend(lock(&self.inner.held).values().map(Held::status));
        out
    }

    /// Waits until this node has everything `peer` had in `project` at
    /// some point after `since`: a pull that went through, not one that
    /// failed, brought its list, and the files are here. `progress` sees
    /// the project's status at each change. Never ends while `peer` is out
    /// of reach.
    pub async fn have_all_from(
        &self,
        project: &str,
        peer: &EndpointId,
        since: Instant,
        mut progress: impl FnMut(&ProjectStatus),
    ) -> Result<()> {
        let Some(shared) = self.project(project) else {
            bail!("{project} is not synced on this machine");
        };
        if !shared.has_peer(peer) {
            bail!("{project} is not synced with that machine");
        }
        let mut status = shared.watch_status();
        if !shared.caught_up_since(peer, since) {
            shared.pull_again();
        }
        loop {
            let now = shared.status();
            progress(&now);
            if shared.caught_up_since(peer, since)
                && now.pending == 0
                && now.waiting.is_empty()
                && !now.paused
            {
                return Ok(());
            }
            tokio::select! {
                changed = status.changed() => if changed.is_err() {
                    bail!("{project} stopped syncing");
                },
                () = shared.stopped() => bail!("{project} stopped syncing"),
            }
        }
    }

    /// What `project`'s sync leaves out, at most `limit` paths.
    pub async fn skipped(&self, project: &str, limit: u32) -> Result<Skipped> {
        let Some(shared) = self.project(project) else {
            bail!("{project} is not synced on this machine");
        };
        let root = shared.root().to_owned();
        let limit = usize::try_from(limit.min(beep_proto::node::sync::MAX_SKIPPED))?;
        let walked = spawn_blocking(move || {
            let rules = rules::Rules::load(&root)?;
            scan::walk(&root, &rules, limit)
        })
        .await??;
        Ok(Skipped {
            paths: walked.skipped,
            through_git: 0,
            truncated: walked.skipped_more,
        })
    }

    /// Settles the conflict copy at `path` of `project`, keeping `keep`.
    pub async fn resolve(&self, project: &str, path: &str, keep: Keep) -> Result<()> {
        let Some(shared) = self.project(project) else {
            bail!("{project} is not synced on this machine");
        };
        shared.resolve(path, keep).await
    }

    /// Scans `project` now and brings it in line, then says how it stands.
    pub async fn rescan(&self, project: &str) -> Result<ProjectStatus> {
        let Some(shared) = self.project(project) else {
            bail!("{project} is not synced on this machine");
        };
        shared.rescan(RESCAN_LIMIT).await
    }

    /// What tells the server this node scans a project when asked.
    pub fn rescans(&self) -> Rescans {
        Rescans(self.clone())
    }

    /// What tells the server this node pauses projects and settles
    /// conflicts.
    pub fn controls(&self) -> Controls {
        Controls(self.clone())
    }

    /// How many pulls other nodes have open here now.
    pub fn pulls_served(&self) -> usize {
        self.inner.serving.load(Ordering::Relaxed)
    }

    fn serving(&self) -> Serving {
        self.inner.serving.fetch_add(1, Ordering::Relaxed);
        Serving(self.inner.serving.clone())
    }

    /// The other nodes `project` syncs with here.
    pub fn peers(&self, project: &str) -> Vec<EndpointId> {
        self.project(project)
            .map(|shared| shared.peer_ids())
            .unwrap_or_default()
    }

    fn project(&self, name: &str) -> Option<Arc<Shared>> {
        lock(&self.inner.projects)
            .get(name)
            .map(|running| running.shared.clone())
    }

    fn is_peer(&self, peer: &EndpointId) -> bool {
        self.inner.may_connect(peer)
    }

    /// Whether `peer` may still make requests: it was not removed from the
    /// mesh.
    fn allows(&self, peer: &EndpointId) -> bool {
        self.inner.allows(peer)
    }
}

/// Checks the folders against Syncthing's whenever it changes one, and at
/// least every minute, while the syncs live. A folder Syncthing resumes is
/// taken away from Beep's sync at once; Syncthing does not wait for that.
async fn check_syncthing(inner: Weak<Inner>, syncthing: Syncthing) {
    let mut since = 0;
    loop {
        syncthing.changed(&mut since).await;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        Syncs { inner }.check_syncthing().await;
    }
}

impl Protocol for Syncs {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(self.clone(), tx, rx))
    }
}

/// `CONTROLS`, offered beside `sync/1`: the server sends a paused project
/// and `Request::Resolve` only to a node that offers it. Its sessions are
/// served as `sync/1`'s.
pub struct Controls(Syncs);

impl Protocol for Controls {
    fn name(&self) -> &str {
        CONTROLS
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(self.0.clone(), tx, rx))
    }
}

/// `RESCAN`, offered beside `sync/1`: the server sends `Request::Rescan`
/// only to a node that offers it. Its sessions are served as `sync/1`'s.
pub struct Rescans(Syncs);

impl Protocol for Rescans {
    fn name(&self) -> &str {
        RESCAN
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(self.0.clone(), tx, rx))
    }
}

/// Serves one `sync/1` request.
async fn serve(syncs: Syncs, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };
    let reply = match request {
        Request::Configure { projects } if syncs.follows_mesh() => Reply::Configured {
            refused: syncs.configure_beside_mesh(projects).await,
            addr: syncs.addr(),
        },
        Request::Configure { projects } => Reply::Configured {
            refused: syncs.configure(projects).await,
            addr: syncs.addr(),
        },
        Request::Status {} => Reply::Status {
            projects: syncs.status(),
        },
        Request::Skipped { project, limit } => match syncs.skipped(&project, limit).await {
            Ok(skipped) => Reply::Skipped { skipped },
            Err(e) => Reply::Error {
                message: format!("{e:#}"),
            },
        },
        Request::Resolve {
            project,
            path,
            keep,
        } => match syncs.resolve(&project, &path, keep).await {
            Ok(()) => Reply::Resolved {},
            Err(e) => Reply::Error {
                message: format!("{e:#}"),
            },
        },
        Request::Rescan { project } => match syncs.rescan(&project).await {
            Ok(status) => Reply::Rescanned { status },
            Err(e) => Reply::Error {
                message: format!("{e:#}"),
            },
        },
        other => Reply::Error {
            message: format!("this node cannot do {other:?} yet"),
        },
    };
    tx.send(&reply).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_mesh::{DeviceKind, JOIN_ALPN, PEER_ALPN, transport_config};
    use iroh::{SecretKey, endpoint::presets, protocol::Router};
    use tempfile::TempDir;

    async fn bind() -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .transport_config(transport_config())
            .bind()
            .await
            .expect("an endpoint binds")
    }

    #[tokio::test]
    async fn a_removed_device_is_refused_on_a_list_the_server_set() {
        // A desktop's mesh with a node that joins it, and no `sync/` key:
        // the server sets the project list.
        let desktop_dir = TempDir::new().expect("a temp folder");
        let desktop = bind().await;
        let mesh = Mesh::create(desktop_dir.path(), desktop.clone(), "desktop")
            .await
            .expect("the mesh is made");
        let (peer, join) = mesh.protocols();
        let _router = Router::builder(desktop)
            .accept(PEER_ALPN, peer)
            .accept(JOIN_ALPN, join)
            .spawn();
        let ticket = mesh
            .invite(DeviceKind::Node, Duration::from_secs(60))
            .await
            .expect("a ticket");
        let (node_dir, node) = (TempDir::new().expect("a temp folder"), bind().await);
        let joined = Mesh::join(node_dir.path(), node.clone(), &ticket, "node")
            .await
            .expect("the node joins");

        let shared = TempDir::new().expect("a temp folder");
        std::fs::create_dir(shared.path().join("proj")).expect("a project folder");
        let state = TempDir::new().expect("a temp folder");
        let here = bind().await;
        let syncthing = Syncthing::at(vec![state.path().join("syncthing")], None);
        let transfers = crate::files::transfer::Transfers::start_with(
            &here,
            state.path(),
            Folders::new(&[shared.path().to_owned()]),
            syncthing,
        )
        .await
        .expect("the transfers start");
        let syncs = transfers.syncs();
        syncs.follow(mesh.clone());
        let refused = syncs
            .configure(vec![Project {
                name: "proj".to_owned(),
                folder: shared
                    .path()
                    .join("proj")
                    .canonicalize()
                    .expect("the folder exists")
                    .display()
                    .to_string(),
                peers: vec![node.addr()],
                paused: false,
            }])
            .await;
        assert!(refused.is_empty(), "{refused:?}");
        let id = node.id();
        assert!(syncs.inner.may_connect(&id), "a peer of the list connects");

        mesh.remove(id).await.expect("the removal is signed");
        assert!(!syncs.inner.may_connect(&id), "a removed device does not");
        joined.close().await.ok();
        transfers.shutdown().await.expect("the node stops");
    }

    #[test]
    fn a_project_in_another_folder_keeps_other_state() {
        let a = key_of("game", Path::new("/srv/project"));
        assert_eq!(a, key_of("game", Path::new("/srv/project")));
        assert_ne!(a, key_of("game", Path::new("/srv/other")));
        assert_ne!(a, key_of("gam", Path::new("e/srv/project")));
    }
}
