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
        CONTROLS, FOLDER_MISSING, Keep, PARENT_MISSING, PROTOCOL, Project, ProjectStatus, RESCAN,
        Refused, Reply, Request, Skipped, State,
    },
};
use futures::future::join_all;
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
mod handoff;
mod ignored;
mod lists;
mod map;
#[cfg(test)]
#[path = "../../tests/files_sync_map_sim.rs"]
mod map_sim;
mod peer;
mod progress;
mod project;
mod root;
mod rules;
mod scan;
pub(super) mod stignore;
mod syncthing;
mod watch;

pub(crate) use peer::{ALPN, PeerHandler};
use project::{Shared, Start};
pub(crate) use rules::Rules;
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
    from: Source,
    shared: Arc<Shared>,
    task: AbortOnDropHandle<()>,
}

/// A project that Syncthing already keeps in step: kept on the list, not
/// synced, until Syncthing lets go of its folder.
struct Held {
    project: Project,
    from: Source,
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

/// A project the mesh or the saved list names that does not run here, and
/// why: kept on the list, reported in the status, and tried again with the
/// folder checks, so a folder that comes back starts syncing.
struct Stopped {
    project: Project,
    from: Source,
    message: String,
}

impl Stopped {
    fn status(&self) -> ProjectStatus {
        ProjectStatus {
            name: self.project.name.clone(),
            folder: self.project.folder.clone(),
            state: State::Failed,
            error: Some(self.message.clone()),
            ..ProjectStatus::default()
        }
    }
}

/// Where a list of projects comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Saved on disk: the server set each of them before.
    Saved,
    /// The server, which may be turning one on now. Keeps to `files`.
    Server,
    /// The signed mesh. A desktop set it, and may run any command on the
    /// machine anyway, so its folder may be anywhere.
    Mesh,
}

struct Inner {
    recoveries: Mutex<BTreeMap<String, std::sync::Weak<crate::agents::Agents>>>,
    parts: Parts,
    folders: Folders,
    /// Where the projects' state is kept.
    dir: PathBuf,
    watcher: Watcher,
    projects: Mutex<BTreeMap<String, Running>>,
    held: Mutex<BTreeMap<String, Held>>,
    stopped: Mutex<BTreeMap<String, Stopped>>,
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
                recoveries: Mutex::default(),
                folders,
                dir,
                watcher,
                projects: Mutex::default(),
                held: Mutex::default(),
                stopped: Mutex::default(),
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

    /// Applies the replicated opt-in. A missing preference always leaves local edits alone.
    pub fn set_git_auto_recovery(
        &self,
        agents: &Arc<crate::agents::Agents>,
        projects: BTreeSet<String>,
    ) {
        let recoveries: BTreeMap<_, _> = projects
            .into_iter()
            .map(|project| (project, Arc::downgrade(agents)))
            .collect();
        *lock(&self.inner.recoveries) = recoveries.clone();

        for (name, running) in lock(&self.inner.projects).iter() {
            running.shared.set_recovery(recoveries.get(name).cloned());
        }
    }

    /// Syncs exactly `projects`, and says which it refused. A project left
    /// out stops; its files stay and its state is forgotten. A folder
    /// Syncthing keeps in step is refused when the server names it new here,
    /// and held when the server set it before or the mesh names it.
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
        lock(&self.inner.recoveries).clear();

        for running in lock(&self.inner.projects).values() {
            running.shared.set_recovery(None);
        }

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
        self.configure_from(projects, Source::Mesh).await
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
            lock(&self.inner.stopped).clear();
        }
        let folders: Vec<PathBuf> = projects.iter().map(|p| PathBuf::from(&p.folder)).collect();
        let managed = self.inner.syncthing.check_all(&folders).await;

        for (mut project, managed) in projects.into_iter().zip(managed) {
            project.peers.retain(|peer| peer.id != me);
            let same = {
                let mut running = lock(&self.inner.projects);
                running
                    .get_mut(&project.name)
                    .filter(|running| running.project.folder == project.folder)
                    .map(|running| {
                        running.project = project.clone();
                        running.from = from;
                        running.shared.clone()
                    })
            };

            if let Some(shared) = same {
                log::debug!("sync {}: keeps running, peers and pause set", project.name);
                shared.set_peers(project.peers.clone());
                shared.set_paused(project.paused);
                kept.push(project);
                continue;
            }
            // Stopped before its replacement starts: the same folder named
            // another way has the same key, watches and state file.
            let old = lock(&self.inner.projects).remove(&project.name);
            if let Some(old) = old {
                log::debug!("sync {}: stopping the run it replaces", project.name);
                replaced.push(halt(old).await);
            }
            log::debug!(
                "sync {}: asking Syncthing about {}",
                project.name,
                project.folder
            );
            let was_held = lock(&self.inner.held)
                .remove(&project.name)
                .is_some_and(|held| held.project.folder == project.folder);
            if let Some(managed) = managed {
                if was_held || from != Source::Server {
                    log::info!("not syncing {}: {}", project.name, managed.reason());
                    lock(&self.inner.held).insert(
                        project.name.clone(),
                        Held {
                            project: project.clone(),
                            from,
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

            log::debug!("sync {}: starting", project.name);

            match self.run(&project, from).await {
                Ok(running) => {
                    lock(&self.inner.projects).insert(project.name.clone(), running);
                    kept.push(project);
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    refused.push(Refused {
                        project: project.name.clone(),
                        message: message.clone(),
                    });
                    // The server is told and decides; what the mesh or the saved list names
                    // stays on the list and is tried again.
                    if from != Source::Server {
                        lock(&self.inner.stopped).insert(
                            project.name.clone(),
                            Stopped {
                                project: project.clone(),
                                from,
                                message,
                            },
                        );
                        kept.push(project);
                    }
                }
            }
        }

        for stopped in stopping {
            log::debug!("sync {}: stopping", stopped.project.name);
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

    async fn run(&self, project: &Project, from: Source) -> Result<Running> {
        let (folders, folder) = (self.inner.folders.clone(), project.folder.clone());
        let ignored = ignored::IgnoredRoot::parse(&project.name, Path::new(&project.folder))?;
        let (root, made) = spawn_blocking(move || {
            let folders = match from {
                Source::Mesh => folders.everywhere(),
                Source::Saved | Source::Server => folders,
            };
            let made = match &ignored {
                Some(scope) => scope.prepare(&folders)?,
                None => make_missing(&folders, &folder)?,
            };
            let root = open_target(&folders, &folder)
                .with_context(|| format!("the folder {folder} cannot be synced"))?;
            if ignored.is_none() && !root.path().join(".git").exists() && root.path().ancestors().skip(1).any(|parent| parent.join(".git").exists()) {
                bail!("This folder is inside a Git checkout. Import the repository root for Git sync.");
            }

            if root.path().join(".git").is_file() {
                bail!("Linked worktrees and submodules stay on their machine. Import the main checkout for Git sync.");
            }

            anyhow::Ok((root, made))
        })
        .await??;
        let key = key_of(&project.name, root.path());

        if made {
            // A folder made again starts as Add's empty folder does: what was seen in the one
            // that went is forgotten, so it is filled from the peers and deletes nothing.
            log::info!(
                "{}: {} was missing, so it is made again and filled from the other machines",
                project.name,
                root.path().display()
            );
            self.forget_key(&project.name, key, &self.state_file(key))
                .await;
        }

        let start = Start {
            name: project.name.clone(),
            key,
            root: Arc::new(root),
            peers: project.peers.clone(),
            file: self.state_file(key),
            paused: project.paused,
        };
        let shared = spawn_blocking(move || Shared::load(start)).await??;
        shared.set_recovery(lock(&self.inner.recoveries).get(&project.name).cloned());
        let task = tokio::spawn(project::run(
            shared.clone(),
            self.inner.parts.clone(),
            self.inner.watcher.clone(),
        ));
        Ok(Running {
            project: project.clone(),
            from,
            shared,
            task: AbortOnDropHandle::new(task),
        })
    }

    async fn stop(&self, running: Running) {
        let shared = halt(running).await;
        self.forget(&shared).await;
    }

    fn state_file(&self, key: Key) -> PathBuf {
        self.inner.dir.join(format!("{key:016x}.state"))
    }

    /// Forgets the state of the stopped project `shared`, unless its folder
    /// runs again under the same key.
    async fn forget(&self, shared: &Shared) {
        self.forget_key(shared.name(), shared.key(), shared.file())
            .await;
    }

    /// Forgets the state kept under `key` in `file`, unless a project runs on it.
    async fn forget_key(&self, name: &str, key: Key, file: &Path) {
        // The same folder may have started again under the same key.
        if lock(&self.inner.projects)
            .values()
            .any(|other| other.shared.key() == key)
        {
            return;
        }
        // A project turned off forgets its folder: started again, it scans
        // from nothing, so a folder emptied meanwhile deletes nothing.
        let file = file.to_owned();
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
        let running: Vec<(Project, Source)> = lock(&self.inner.projects)
            .values()
            .map(|running| (running.project.clone(), running.from))
            .collect();
        let folders: Vec<PathBuf> = running
            .iter()
            .map(|(p, _)| PathBuf::from(&p.folder))
            .collect();
        let managed = self.inner.syncthing.check_all(&folders).await;

        for ((project, from), managed) in running.into_iter().zip(managed) {
            let Some(managed) = managed else {
                continue;
            };
            log::info!("stopped syncing {}: {}", project.name, managed.reason());
            let stopped = lock(&self.inner.projects).remove(&project.name);

            if let Some(stopped) = stopped {
                self.stop(stopped).await;
            }

            lock(&self.inner.held).insert(
                project.name.clone(),
                Held {
                    project,
                    from,
                    managed,
                },
            );
        }

        let held: Vec<(Project, Source)> = lock(&self.inner.held)
            .values()
            .map(|held| (held.project.clone(), held.from))
            .collect();
        let folders: Vec<PathBuf> = held.iter().map(|(p, _)| PathBuf::from(&p.folder)).collect();
        let managed = self.inner.syncthing.check_all(&folders).await;

        for ((project, from), managed) in held.into_iter().zip(managed) {
            if managed.is_some() {
                continue;
            }

            match self.run(&project, from).await {
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

    /// Checks the projects' folders now: a running project whose folder is gone stops (its scan
    /// already refuses to send deletes) and starts again, which makes the folder and fills it
    /// from the peers; a project that did not start is tried again, so a folder that is back,
    /// or whose rules can be followed now, syncs.
    pub async fn check_folders(&self) {
        let _one = self.inner.configuring.lock().await;
        let running: Vec<(Project, Source)> = lock(&self.inner.projects)
            .values()
            .map(|running| (running.project.clone(), running.from))
            .collect();
        let mut again: Vec<(Project, Source)> = Vec::new();

        for (project, from) in running {
            let folder = PathBuf::from(&project.folder);

            if spawn_blocking(move || folder.exists())
                .await
                .unwrap_or(true)
            {
                continue;
            }

            log::info!("{}: its folder {} is gone", project.name, project.folder);
            let stopped = lock(&self.inner.projects).remove(&project.name);

            if let Some(stopped) = stopped {
                self.stop(stopped).await;
            }

            again.push((project, from));
        }

        again.extend(
            lock(&self.inner.stopped)
                .values()
                .map(|stopped| (stopped.project.clone(), stopped.from)),
        );

        for (project, from) in again {
            match self.run(&project, from).await {
                Ok(running) => {
                    log::info!("syncing {} again", project.name);
                    lock(&self.inner.stopped).remove(&project.name);
                    lock(&self.inner.projects).insert(project.name.clone(), running);
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    let mut stopped = lock(&self.inner.stopped);

                    if stopped
                        .get(&project.name)
                        .is_none_or(|stopped| stopped.message != message)
                    {
                        log::warn!("not syncing {}: {message}", project.name);
                    }

                    stopped.insert(
                        project.name.clone(),
                        Stopped {
                            project,
                            from,
                            message,
                        },
                    );
                }
            }
        }
    }

    /// Stops every project, saving its state.
    pub(crate) async fn shutdown(&self) {
        let checking = lock(&self.inner.checking).take();
        drop(checking);
        let _one = self.inner.configuring.lock().await;
        let running = std::mem::take(&mut *lock(&self.inner.projects));
        // All at once: one busy project holds up no other's save.
        join_all(running.into_values().map(halt)).await;
    }

    /// How each project stands.
    pub fn status(&self) -> Vec<ProjectStatus> {
        let mut out: Vec<ProjectStatus> = lock(&self.inner.projects)
            .values()
            .map(|running| running.shared.status())
            .collect();
        out.extend(lock(&self.inner.held).values().map(Held::status));
        out.extend(lock(&self.inner.stopped).values().map(Stopped::status));
        out
    }

    /// A project's Git and selected ignored-folder datasets as one status.
    pub(crate) fn status_for(&self, project: &str) -> Option<ProjectStatus> {
        let all = self.status();
        let main = all.iter().find(|status| status.name == project)?;
        let folders = all
            .iter()
            .filter_map(|status| {
                let (owner, path) = beep_proto::node::sync::ignored_folder(&status.name)?;
                (owner == project).then(|| path.to_owned())
            })
            .collect::<Vec<_>>();
        Some(
            beep_proto::node::sync::IgnoredSync {
                enabled: !folders.is_empty(),
                folders,
            }
            .status(main, &all),
        )
    }

    /// The projects waiting for Syncthing to let go of their folder.
    pub(crate) fn held(&self) -> BTreeSet<String> {
        lock(&self.inner.held).keys().cloned().collect()
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
            // An error in the status, such as two checkouts at different
            // commits or a folder that cannot be checked, is a reason the
            // files are not all here, whatever the counts say.
            if shared.caught_up_since(peer, since)
                && now.pending == 0
                && now.waiting.is_empty()
                && !now.paused
                && now.error.is_none()
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
        let mut datasets = vec![shared];
        datasets.extend(lock(&self.inner.projects).values().filter_map(|running| {
            let (owner, _) = beep_proto::node::sync::ignored_folder(&running.project.name)?;
            (owner == project).then(|| running.shared.clone())
        }));

        for result in join_all(datasets.iter().map(|shared| shared.rescan(RESCAN_LIMIT))).await {
            result?;
        }

        self.status_for(project)
            .with_context(|| format!("{project} stopped syncing"))
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

    /// Whether Beep syncs `project` here with `peer`, once a change to the
    /// list that is going on is done.
    pub(crate) async fn syncs_with(&self, project: &str, peer: &EndpointId) -> bool {
        let _settled = self.inner.configuring.lock().await;
        self.project(project)
            .is_some_and(|shared| shared.has_peer(peer))
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
        let syncs = Syncs { inner };
        syncs.check_syncthing().await;
        syncs.check_folders().await;
    }
}

/// Makes `folder` when it is missing and the folder above it is there, and says whether it did.
/// With the folder above missing too, as when a disk is not mounted, nothing is made.
fn make_missing(folders: &Folders, folder: &str) -> Result<bool> {
    let path = Path::new(folder);

    match fs::symlink_metadata(path) {
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("reading {folder}")),
    }

    if !path.parent().is_some_and(Path::is_dir) {
        bail!("{PARENT_MISSING}: {folder}");
    }

    let resolved = folders
        .resolve(folder)
        .with_context(|| format!("{FOLDER_MISSING}: {folder}"))?;
    fs::create_dir(&resolved).with_context(|| format!("{FOLDER_MISSING}: making {folder}"))?;
    Ok(true)
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
        Request::HandoffSnapshot { project } => match syncs.handoff_snapshot(&project).await {
            Ok(snapshot) => Reply::HandoffSnapshot { snapshot },
            Err(e) => Reply::Error {
                message: format!("{e:#}"),
            },
        },
        Request::HandoffApply { project, snapshot } => {
            match syncs.handoff_apply(&project, snapshot).await {
                Ok(()) => Reply::HandoffApplied {},
                Err(e) => Reply::Error {
                    message: format!("{e:#}"),
                },
            }
        }
        Request::SyncthingFiles { folder, enabled } => {
            match syncs.syncthing_files(&folder, enabled).await {
                Ok(files) => Reply::SyncthingFiles { files },
                Err(e) => Reply::Error {
                    message: format!("{e:#}"),
                },
            }
        }
        other => Reply::Error {
            message: format!("this node cannot do {other:?} yet"),
        },
    };
    tx.send(&reply).await
}

#[cfg(test)]
#[path = "../../tests/files_sync.rs"]
mod tests;
