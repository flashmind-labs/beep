//! `sync/1` (see `beep_proto::node::sync`): keeping project folders in step
//! with the same project on the user's other machines.
//!
//! The server only says which projects to sync, where, and with which
//! peers; this node keeps that list on disk so sync goes on while the
//! server is down. Each project's file list is exchanged node to node
//! (`peer`), and files move over iroh-blobs from the store transfers use,
//! from any peer that holds them, to the peers of that project only.

use crate::{
    AsyncMutex,
    files::Folders,
    link::{Protocol, Session},
    syncthing::{Managed, Syncthing},
    transfer::{
        grants::Access,
        place::{Journal, Room, open_target},
    },
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    sync::{PROTOCOL, Project, ProjectStatus, Refused, Reply, Request, Skipped, State},
};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_blobs::{Hash, store::fs::FsStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    task::spawn_blocking,
    time::{MissedTickBehavior, interval, timeout},
};
use tokio_util::task::AbortOnDropHandle;

mod disk;
mod map;
mod peer;
mod project;
mod root;
mod rules;
mod scan;
mod watch;

pub(crate) use peer::{ALPN, PeerHandler};
use project::{Shared, Start};
use watch::{Key, Watcher};

/// How long a project that stops may take to save its state.
const STOP_LIMIT: Duration = Duration::from_secs(10);
/// How often the synced folders are checked against Syncthing's.
const SYNCTHING_CHECK: Duration = Duration::from_secs(60);
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

impl Access for Inner {
    fn may_connect(&self, peer: &EndpointId) -> bool {
        self.lock()
            .values()
            .any(|running| running.shared.has_peer(peer))
    }

    fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool {
        self.lock()
            .values()
            .any(|running| running.shared.has_peer(peer) && running.shared.has(hash))
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Running>> {
        self.projects.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn held(&self) -> MutexGuard<'_, BTreeMap<String, Held>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn checking(&self) -> MutexGuard<'_, Option<AbortOnDropHandle<()>>> {
        self.checking.lock().unwrap_or_else(PoisonError::into_inner)
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
            }),
        };
        for refused in syncs.configure_from(config.projects, Source::Saved).await {
            log::warn!("not syncing {}: {}", refused.project, refused.message);
        }
        let task = tokio::spawn(check_syncthing(Arc::downgrade(&syncs.inner)));
        *syncs.inner.checking() = Some(AbortOnDropHandle::new(task));
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
        self.configure_from(projects, Source::Server).await
    }

    async fn configure_from(&self, projects: Vec<Project>, from: Source) -> Vec<Refused> {
        let _one = self.inner.configuring.lock().await;
        let me = self.inner.parts.endpoint.id();
        let mut refused = Vec::new();
        let mut kept = Vec::new();
        let mut stopping = Vec::new();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        {
            let mut running = self.inner.lock();
            let gone: Vec<String> = running
                .keys()
                .filter(|name| !names.contains(&name.as_str()))
                .cloned()
                .collect();
            for name in gone {
                stopping.extend(running.remove(&name));
            }
            self.inner
                .held()
                .retain(|name, _| names.contains(&name.as_str()));
        }
        for mut project in projects {
            project.peers.retain(|peer| peer.id != me);
            let same = {
                let mut running = self.inner.lock();
                running
                    .get_mut(&project.name)
                    .filter(|running| running.project.folder == project.folder)
                    .map(|running| {
                        running.project.peers = project.peers.clone();
                        running.shared.clone()
                    })
            };
            if let Some(shared) = same {
                shared.set_peers(project.peers.clone());
                kept.push(project);
                continue;
            }
            stopping.extend(self.inner.lock().remove(&project.name));
            let was_held = self
                .inner
                .held()
                .remove(&project.name)
                .is_some_and(|held| held.project.folder == project.folder);
            if let Some(managed) = self
                .inner
                .syncthing
                .managed(Path::new(&project.folder))
                .await
            {
                if was_held || from == Source::Saved {
                    log::info!("not syncing {}: {}", project.name, managed.reason());
                    self.inner.held().insert(
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
                    self.inner.lock().insert(project.name.clone(), running);
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
        let config = Config { projects: kept };
        let saved = serde_json::to_vec_pretty(&config)
            .context("encoding the projects")
            .and_then(|bytes| {
                let file = self.inner.dir.join(CONFIG);
                let temp = file.with_extension("tmp");
                fs::write(&temp, bytes)?;
                fs::rename(&temp, &file)?;
                Ok(())
            });
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
        running.shared.stop();
        let name = running.shared.name().to_owned();
        if timeout(STOP_LIMIT, running.task).await.is_err() {
            log::warn!("{name} did not stop in time");
        }
        let key = running.shared.key();
        // The same folder may have started again under the same key.
        if self
            .inner
            .lock()
            .values()
            .any(|other| other.shared.key() == key)
        {
            return;
        }
        // A project turned off forgets its folder: started again, it scans
        // from nothing, so a folder emptied meanwhile deletes nothing.
        let file = running.shared.file().to_owned();
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
    /// Syncthing let go of starts again, from a fresh scan.
    pub async fn check_syncthing(&self) {
        let _one = self.inner.configuring.lock().await;
        let running: Vec<Project> = self
            .inner
            .lock()
            .values()
            .map(|running| running.project.clone())
            .collect();
        for project in running {
            let Some(managed) = self.inner.syncthing.check(Path::new(&project.folder)).await else {
                continue;
            };
            log::info!("stopped syncing {}: {}", project.name, managed.reason());
            let stopped = self.inner.lock().remove(&project.name);
            self.inner
                .held()
                .insert(project.name.clone(), Held { project, managed });
            if let Some(stopped) = stopped {
                self.stop(stopped).await;
            }
        }
        let held: Vec<Project> = self
            .inner
            .held()
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
                    log::info!("syncing {} again: Syncthing let go of it", project.name);
                    self.inner.held().remove(&project.name);
                    self.inner.lock().insert(project.name.clone(), running);
                }
                Err(e) => log::warn!("{} does not sync yet: {e:#}", project.name),
            }
        }
    }

    /// Stops every project, saving its state.
    pub(crate) async fn shutdown(&self) {
        let checking = self.inner.checking().take();
        drop(checking);
        let _one = self.inner.configuring.lock().await;
        let running = std::mem::take(&mut *self.inner.lock());
        for running in running.into_values() {
            running.shared.stop();
            let name = running.shared.name().to_owned();
            if timeout(STOP_LIMIT, running.task).await.is_err() {
                log::warn!("{name} did not stop in time");
            }
        }
    }

    /// How each project stands.
    pub fn status(&self) -> Vec<ProjectStatus> {
        let mut out: Vec<ProjectStatus> = self
            .inner
            .lock()
            .values()
            .map(|running| running.shared.status())
            .collect();
        out.extend(self.inner.held().values().map(Held::status));
        out
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
            through_git: walked.through_git,
            truncated: walked.skipped_more,
        })
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
        self.inner
            .lock()
            .get(name)
            .map(|running| running.shared.clone())
    }

    fn is_peer(&self, peer: &EndpointId) -> bool {
        self.inner.may_connect(peer)
    }
}

/// Checks the folders against Syncthing's every `SYNCTHING_CHECK` while
/// the syncs live.
async fn check_syncthing(inner: Weak<Inner>) {
    let mut every = interval(SYNCTHING_CHECK);
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // Starting checked them already.
    every.tick().await;
    loop {
        every.tick().await;
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

/// Serves one `sync/1` request.
async fn serve(syncs: Syncs, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };
    let reply = match request {
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
        other => Reply::Error {
            message: format!("this node cannot do {other:?} yet"),
        },
    };
    tx.send(&reply).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_in_another_folder_keeps_other_state() {
        let a = key_of("game", Path::new("/srv/project"));
        assert_eq!(a, key_of("game", Path::new("/srv/project")));
        assert_ne!(a, key_of("game", Path::new("/srv/other")));
        assert_ne!(a, key_of("gam", Path::new("e/srv/project")));
    }
}
