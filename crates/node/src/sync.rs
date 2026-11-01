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
    transfer::{
        grants::Access,
        place::{Journal, Room, open_target},
    },
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    sync::{PROTOCOL, Project, ProjectStatus, Refused, Reply, Request, Skipped},
};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_blobs::{Hash, store::fs::FsStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{task::spawn_blocking, time::timeout};
use tokio_util::task::AbortOnDropHandle;

mod disk;
mod map;
mod peer;
mod project;
mod rules;
mod scan;
mod watch;

pub(crate) use peer::{ALPN, PeerHandler};
use project::{Shared, Start};
use watch::{Key, Watcher};

/// How long a project that stops may take to save its state.
const STOP_LIMIT: Duration = Duration::from_secs(10);
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
    folder: String,
    shared: Arc<Shared>,
    task: AbortOnDropHandle<()>,
}

struct Inner {
    parts: Parts,
    folders: Folders,
    /// Where the projects' state is kept.
    dir: PathBuf,
    watcher: Watcher,
    projects: Mutex<BTreeMap<String, Running>>,
    /// One configure at a time.
    configuring: AsyncMutex<()>,
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
    /// Starts the projects saved in `dir`, as the server last set them.
    pub(crate) async fn start(parts: Parts, folders: Folders, dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        let config = match fs::read(dir.join(CONFIG)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("starting with no synced projects: {CONFIG} is unreadable: {e}");
                Config::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {CONFIG}")),
        };
        let syncs = Self {
            inner: Arc::new(Inner {
                parts,
                folders,
                dir,
                watcher: Watcher::new(),
                projects: Mutex::default(),
                configuring: AsyncMutex::new(()),
            }),
        };
        for refused in syncs.configure(config.projects).await {
            log::warn!("not syncing {}: {}", refused.project, refused.message);
        }
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
    /// out stops; its files and state stay.
    pub async fn configure(&self, projects: Vec<Project>) -> Vec<Refused> {
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
        }
        for mut project in projects {
            project.peers.retain(|peer| peer.id != me);
            let same = {
                let running = self.inner.lock();
                running
                    .get(&project.name)
                    .filter(|running| running.folder == project.folder)
                    .map(|running| running.shared.clone())
            };
            if let Some(shared) = same {
                shared.set_peers(project.peers.clone());
                kept.push(project);
                continue;
            }
            stopping.extend(self.inner.lock().remove(&project.name));
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
            folder: project.folder.clone(),
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
        let key = key_of(&name, running.shared.root());
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

    /// Stops every project, saving its state.
    pub(crate) async fn shutdown(&self) {
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
        self.inner
            .lock()
            .values()
            .map(|running| running.shared.status())
            .collect()
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
