//! `sync/1` (see `beep_proto::node::sync`): keeps project folders in step
//! with the same project on the user's other machines.

use crate::{
    files::Folders,
    files::place::{Journal, Room, open_target},
    files::syncthing::{Managed, Syncthing},
    lock,
    membership::Membership,
    protocol::{Protocol, Session},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    sync::{
        CONTROLS, FOLDER_MISSING, Keep, PARENT_MISSING, PROTOCOL, Project, ProjectStatus, RESCAN,
        Refused, Reply, Request, Skipped,
    },
};
use futures::future::join_all;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_blobs::{Hash, store::fs::FsStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
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

mod actor;
mod disk;
mod gitconfig;
mod gitdir;
mod handoff;
mod ignored;
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
mod syncthing;
mod watch;

use actor::{Entry, Phase, Projects};
use peer::SyncPool;
pub(crate) use peer::{ALPN, PeerHandler};
use project::{Shared, Start};
pub(crate) use rules::Rules;
use watch::{Key, Watcher};

/// How long a scan `Request::Rescan` asks for may take.
const RESCAN_LIMIT: Duration = Duration::from_secs(120);
/// How long a transfer may wait for its next data before it gives up. A
/// connection can stay open on a path that no longer carries data (a NAT
/// mapping gone, with the relay keeping the connection alive); without a
/// limit such a transfer waits forever. The next round tries again over a
/// new connection and keeps what came.
const STALL: Duration = Duration::from_secs(30);
/// What a change asked for while the node stops is told.
const STOPPING: &str = "this node is stopping";
/// What a change that ended without an answer, as by a panic, tells.
const FAILED: &str = "the change to the synced projects failed";
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

/// A project's run: its state and the task that syncs it.
struct Running {
    shared: Arc<Shared>,
    task: AbortOnDropHandle<()>,
}

/// Where a list of projects comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Set directly or saved on disk: keeps to `folders`.
    Direct,
    /// The signed mesh. A desktop set it, and may run any command on the
    /// machine anyway, so its folder may be anywhere.
    Mesh,
}

struct Inner {
    /// The projects that opted in to automatic Git recovery.
    recoveries: Mutex<BTreeSet<String>>,
    /// The checkouts waiting for recovery to be let run.
    asks: crate::files::holds::Recoveries,
    parts: Parts,
    folders: Folders,
    /// Where the projects' state is kept.
    dir: PathBuf,
    watcher: Watcher,
    /// Every project's state.
    projects: Projects,
    syncthing: Syncthing,
    /// Checks the folders against Syncthing's every so often.
    checking: Mutex<Option<AbortOnDropHandle<()>>>,
    /// The pulls other nodes have open here.
    serving: Arc<AtomicUsize>,
    /// The mesh this node is in, once it is in one: it names the projects,
    /// and its device list says who may sync them.
    mesh: Membership,
    /// Shared `beep-sync/7` connection per peer across all synced projects.
    sync_pool: SyncPool,
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

/// What `Syncs::changes` hands out.
pub(crate) struct SyncChanges(tokio::sync::watch::Receiver<actor::Seen>);

impl SyncChanges {
    /// Returns when the projects changed since the last call.
    pub(crate) async fn changed(&mut self) {
        // The projects live as long as the node; a closed watch never wakes.
        if self.0.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
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
        self.mesh.get().is_some_and(|mesh| mesh.is_removed(*peer))
    }
}

impl Inner {
    /// Whether `peer` may fetch anything at all: a device the node's mesh
    /// lists as removed may not, whatever is open for it.
    fn allows(&self, peer: &EndpointId) -> bool {
        !self.removed(peer)
    }

    fn may_connect(&self, peer: &EndpointId) -> bool {
        self.allows(peer)
            && self
                .projects
                .seen()
                .values()
                .filter_map(Entry::running)
                .any(|shared| shared.has_peer(peer))
    }

    fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool {
        self.allows(peer)
            && self
                .projects
                .seen()
                .values()
                .filter_map(Entry::running)
                .any(|shared| shared.has_peer(peer) && shared.has(hash))
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
        mesh: Membership,
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
                asks: crate::files::holds::Recoveries::default(),
                folders,
                dir,
                watcher,
                projects: Projects::default(),
                syncthing,
                checking: Mutex::default(),
                serving: Arc::default(),
                mesh,
                sync_pool: SyncPool::default(),
            }),
        };

        for refused in syncs
            .configure_from(config.projects, Source::Direct, true)
            .await
        {
            log::warn!("not syncing {}: {}", refused.project, refused.message);
        }

        let task = tokio::spawn(check_syncthing(
            Arc::downgrade(&syncs.inner),
            syncs.inner.syncthing.clone(),
        ));
        *lock(&syncs.inner.checking) = Some(AbortOnDropHandle::new(task));
        Ok(syncs)
    }

    /// Whether `peer` may fetch the blob `hash` of a synced project.
    pub(crate) fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool {
        self.inner.may_get(peer, hash)
    }

    /// How other nodes reach this one.
    pub fn addr(&self) -> EndpointAddr {
        self.inner.parts.endpoint.addr()
    }

    /// Applies the replicated opt-in. A missing preference always leaves local edits alone.
    /// A project that opted in publishes the checkout in `recoveries` when
    /// local edits block it, and recovers when `answer_recovery` holds it.
    pub fn set_git_auto_recovery(&self, projects: BTreeSet<String>) {
        *lock(&self.inner.recoveries) = projects.clone();

        for (name, entry) in self.inner.projects.seen().iter() {
            if let Some(shared) = entry.running() {
                shared.set_recovery(self.recovery_of(name));
            }
        }
    }

    /// The checkouts whose local edits wait for recovery to be let run.
    pub fn recoveries(&self) -> tokio::sync::watch::Receiver<BTreeSet<PathBuf>> {
        self.inner.asks.subscribe()
    }

    /// Answers the checkout `root` that waits in `recoveries`: the hold that
    /// lets its recovery run, which sync releases when it ends, or why not.
    pub fn answer_recovery(
        &self,
        root: &Path,
        answer: anyhow::Result<crate::files::holds::RepositoryHold>,
    ) {
        let shared = self
            .inner
            .projects
            .seen()
            .values()
            .filter_map(|entry| entry.running().cloned())
            .find(|shared| shared.root() == root);
        self.inner.asks.answered(root);

        if let Some(shared) = shared {
            shared.answer_recovery(answer);
        }
    }

    fn recovery_of(&self, project: &str) -> Option<crate::files::holds::Recoveries> {
        lock(&self.inner.recoveries)
            .contains(project)
            .then(|| self.inner.asks.clone())
    }

    /// Syncs exactly `projects`, and says which it refused. A project left
    /// out stops; its files stay and its state is forgotten.
    pub async fn configure(&self, projects: Vec<Project>) -> Vec<Refused> {
        self.configure_from(projects, Source::Direct, false).await
    }

    /// The node left its mesh: the recovery opt-ins it carried go.
    pub(crate) fn forget_recoveries(&self) {
        lock(&self.inner.recoveries).clear();

        for shared in self
            .inner
            .projects
            .seen()
            .values()
            .filter_map(Entry::running)
        {
            shared.set_recovery(None);
        }
    }

    /// Syncs the projects the mesh names.
    pub(crate) async fn configure_from_mesh(&self, projects: Vec<Project>) -> Vec<Refused> {
        self.configure_from(projects, Source::Mesh, true).await
    }

    async fn configure_from(
        &self,
        projects: Vec<Project>,
        from: Source,
        keep: bool,
    ) -> Vec<Refused> {
        let names: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();

        self.change("configure", move |syncs| async move {
            syncs.do_configure(projects, from, keep).await
        })
        .await
        .unwrap_or_else(|why| {
            names
                .into_iter()
                .map(|project| Refused {
                    project,
                    message: why.to_owned(),
                })
                .collect()
        })
    }

    async fn do_configure(&self, projects: Vec<Project>, from: Source, keep: bool) -> Vec<Refused> {
        let state = &self.inner.projects;
        let _list = state.list.lock().await;
        let me = self.inner.parts.endpoint.id();
        let dropped: Vec<String> = state
            .seen()
            .keys()
            .filter(|name| !projects.iter().any(|project| project.name == **name))
            .cloned()
            .collect();
        // Each project halts and loads on its own, all together.
        let dropping = join_all(dropped.iter().map(|name| self.drop_project(name)));
        let loading = join_all(projects.into_iter().map(|mut project| {
            project.peers.retain(|peer| peer.id != me);
            self.load_project(project, from, keep)
        }));
        let (_, loaded) = futures::join!(dropping, loading);
        let mut refused = Vec::new();
        let mut kept = Vec::new();

        for (project, no) in loaded {
            kept.extend(project);
            refused.extend(no);
        }

        // A node that stops refused the loads: its list stays as it was.
        if state.closing() {
            return refused;
        }

        let config = Config { projects: kept };
        let saved = serde_json::to_vec_pretty(&config)
            .context("encoding the projects")
            .and_then(|bytes| crate::fs::note::write(&self.inner.dir.join(CONFIG), &bytes));
        if let Err(e) = saved {
            log::warn!("the synced projects are not saved: {e:#}");
        }

        refused
    }

    /// Stops a project the list no longer names, and forgets it.
    async fn drop_project(&self, name: &str) {
        let state = &self.inner.projects;
        let _gate = state.gate(name).await;

        match state.take_dropped(name) {
            Some(running) => {
                log::debug!("sync {name}: stopping");
                self.stop(running).await;
            }
            None => state.remove(&[name.to_owned()]),
        }
    }

    /// Brings one project of the list to what the list says: the project
    /// when it stays listed, and why it is refused, if it is.
    async fn load_project(
        &self,
        project: Project,
        from: Source,
        keep: bool,
    ) -> (Option<Project>, Option<Refused>) {
        let state = &self.inner.projects;
        let _gate = state.gate(&project.name).await;

        if let Some(shared) = state.keep(&project, from) {
            log::debug!("sync {}: keeps running, peers and pause set", project.name);
            shared.set_peers(project.peers.clone());
            shared.set_paused(project.paused);

            return (Some(project), None);
        }

        // Asked under the gate: Syncthing may have taken the folder while
        // this waited for it.
        let managed = self.inner.syncthing.check(Path::new(&project.folder)).await;
        // Stopped before its replacement starts: the same folder named
        // another way has the same key, watches and state file.
        let replaced = match state.take(&project.name) {
            Some(old) => {
                log::debug!("sync {}: stopping the run it replaces", project.name);
                Some(halt(old).await)
            }
            None => None,
        };
        let loaded = self.start_listed(project, managed, from, keep).await;

        if let Some(old) = replaced {
            self.forget(&old).await;
        }

        loaded
    }

    async fn start_listed(
        &self,
        project: Project,
        managed: Option<Managed>,
        from: Source,
        keep: bool,
    ) -> (Option<Project>, Option<Refused>) {
        let state = &self.inner.projects;
        log::debug!(
            "sync {}: asking Syncthing about {}",
            project.name,
            project.folder
        );
        let was_held = state.is(&project, Phase::is_held);

        if let Some(managed) = managed {
            if was_held || keep {
                log::info!("not syncing {}: {}", project.name, managed.reason());
                state.put(&project, from, Phase::Held(managed.reason()));

                return (Some(project), None);
            }

            state.remove(std::slice::from_ref(&project.name));
            let refused = Refused {
                project: project.name.clone(),
                message: managed.reason(),
            };

            return (None, Some(refused));
        }

        log::debug!("sync {}: starting", project.name);
        // One that was held or stopped stays listed as it was until it runs.
        if !state
            .seen()
            .get(&project.name)
            .is_some_and(|entry| matches!(entry.phase, Phase::Held(_) | Phase::Stopped(_)))
        {
            state.put(&project, from, Phase::Starting);
        }

        match self.run(&project, from).await {
            Ok(running) => {
                self.started(&project, from, running);

                (Some(project), None)
            }
            Err(e) => {
                let message = format!("{e:#}");
                let refused = Refused {
                    project: project.name.clone(),
                    message: message.clone(),
                };

                if keep {
                    state.put(&project, from, Phase::Stopped(message));

                    (Some(project), Some(refused))
                } else {
                    state.remove(std::slice::from_ref(&project.name));

                    (None, Some(refused))
                }
            }
        }
    }

    /// Lists a run that loaded as running.
    fn started(&self, project: &Project, from: Source, running: Running) {
        let shared = running.shared.clone();
        self.inner.projects.start_run(project, from, running);
        // Again now it is listed: opt-ins that changed while it loaded
        // find a run listed as running, or are read here.
        shared.set_recovery(self.recovery_of(&project.name));
    }

    async fn run(&self, project: &Project, from: Source) -> Result<Running> {
        // A node that stops loads nothing: a run that halts still saves its state.
        if self.inner.projects.closing() {
            bail!(STOPPING);
        }

        let (folders, folder) = (self.inner.folders.clone(), project.folder.clone());
        let ignored = ignored::IgnoredRoot::parse(&project.name, Path::new(&project.folder))?;
        let (root, made) = spawn_blocking(move || {
            let folders = match from {
                Source::Mesh => folders.everywhere(),
                Source::Direct => folders,
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
            rules: project.rules.clone(),
        };
        let shared = spawn_blocking(move || Shared::load(start)).await??;
        shared.set_recovery(self.recovery_of(&project.name));
        let task = tokio::spawn(project::run(
            shared.clone(),
            self.inner.parts.clone(),
            self.inner.watcher.clone(),
            self.inner.sync_pool.clone(),
            self.inner.mesh.clone(),
        ));
        Ok(Running {
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
        if self.inner.projects.closing()
            || self
                .inner
                .projects
                .seen()
                .values()
                .filter_map(Entry::running)
                .any(|other| other.key() == key)
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
        self.change("check_syncthing", |syncs| async move {
            syncs.do_check_syncthing().await;
        })
        .await
        .ok();
    }

    async fn do_check_syncthing(&self) {
        let running = self.entries(Phase::is_running);
        let folders: Vec<PathBuf> = running
            .iter()
            .map(|entry| PathBuf::from(&entry.project.folder))
            .collect();
        let managed = self.inner.syncthing.check_all(&folders).await;
        join_all(
            running
                .into_iter()
                .zip(managed)
                .filter_map(|(entry, managed)| Some(self.hold(entry, managed?))),
        )
        .await;

        let held = self.entries(Phase::is_held);
        let folders: Vec<PathBuf> = held
            .iter()
            .map(|entry| PathBuf::from(&entry.project.folder))
            .collect();
        let managed = self.inner.syncthing.check_all(&folders).await;
        join_all(
            held.into_iter()
                .zip(managed)
                .filter(|(_, managed)| managed.is_none())
                .map(|(entry, _)| self.resume(entry)),
        )
        .await;
    }

    /// Stops a running project whose folder Syncthing keeps in step.
    async fn hold(&self, entry: Entry, managed: Managed) {
        let state = &self.inner.projects;
        let Entry { project, from, .. } = entry;
        let _gate = state.gate(&project.name).await;

        // Another change took it while this one waited.
        if !state.is(&project, Phase::is_running) {
            return;
        }

        log::info!("stopped syncing {}: {}", project.name, managed.reason());

        if let Some(stopped) = state.take(&project.name) {
            self.stop(stopped).await;
        }

        state.put(&project, from, Phase::Held(managed.reason()));
    }

    /// Starts a held project whose folder Syncthing let go of or paused.
    async fn resume(&self, entry: Entry) {
        let state = &self.inner.projects;
        let _gate = state.gate(&entry.project.name).await;

        if !state.is(&entry.project, Phase::is_held) {
            return;
        }

        match self.run(&entry.project, entry.from).await {
            Ok(running) => {
                log::info!(
                    "syncing {}: Syncthing let go of it or paused it",
                    entry.project.name
                );
                self.started(&entry.project, entry.from, running);
            }
            Err(e) => log::warn!("{} does not sync yet: {e:#}", entry.project.name),
        }
    }

    /// Checks the projects' folders now: a running project whose folder is gone stops (its scan
    /// already refuses to send deletes) and starts again, which makes the folder and fills it
    /// from the peers; a project that did not start is tried again, so a folder that is back,
    /// or whose rules can be followed now, syncs.
    pub async fn check_folders(&self) {
        self.change("check_folders", |syncs| async move {
            syncs.do_check_folders().await;
        })
        .await
        .ok();
    }

    async fn do_check_folders(&self) {
        let names: Vec<String> = self
            .inner
            .projects
            .seen()
            .iter()
            .filter(|(_, entry)| entry.phase.is_running() || entry.phase.is_stopped())
            .map(|(name, _)| name.clone())
            .collect();
        join_all(names.iter().map(|name| self.check_folder(name))).await;
    }

    async fn check_folder(&self, name: &str) {
        let state = &self.inner.projects;
        let _gate = state.gate(name).await;
        // Read under the gate: the project may have changed while it waited.
        let Some(entry) = state.seen().get(name).cloned() else {
            return;
        };
        let Entry {
            project,
            from,
            phase,
        } = entry;

        if phase.is_running() {
            let folder = PathBuf::from(&project.folder);

            if spawn_blocking(move || folder.exists())
                .await
                .unwrap_or(true)
            {
                return;
            }

            log::info!("{}: its folder {} is gone", project.name, project.folder);

            if let Some(stopped) = state.take(&project.name) {
                self.stop(stopped).await;
            }

            state.put(&project, from, Phase::Starting);
        } else if !phase.is_stopped() {
            return;
        }

        match self.run(&project, from).await {
            Ok(running) => {
                log::info!("syncing {} again", project.name);
                self.started(&project, from, running);
            }
            Err(e) => {
                let message = format!("{e:#}");
                let same = state.seen().get(&project.name).is_some_and(
                    |entry| matches!(&entry.phase, Phase::Stopped(last) if *last == message),
                );

                if !same {
                    log::warn!("not syncing {}: {message}", project.name);
                }

                state.put(&project, from, Phase::Stopped(message));
            }
        }
    }

    /// The projects now in a phase `of` names, by name.
    fn entries(&self, of: fn(&Phase) -> bool) -> Vec<Entry> {
        self.inner
            .projects
            .seen()
            .values()
            .filter(|entry| of(&entry.phase))
            .cloned()
            .collect()
    }

    /// Stops every project, saving its state. The projects halt at once,
    /// not after a change in progress.
    pub(crate) async fn shutdown(&self) {
        let checking = lock(&self.inner.checking).take();
        drop(checking);
        self.inner.projects.shutdown().await;
    }

    /// How each project stands. A project in the middle of a change is
    /// still listed.
    pub fn status(&self) -> Vec<ProjectStatus> {
        self.inner
            .projects
            .seen()
            .values()
            .map(Entry::status)
            .collect()
    }

    /// Wakes when a project is added, removed or changes phase. A status
    /// change inside a running project rings the `sync` topic instead.
    pub(crate) fn changes(&self) -> SyncChanges {
        SyncChanges(self.inner.projects.subscribe())
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
        self.inner
            .projects
            .seen()
            .iter()
            .filter(|(_, entry)| matches!(entry.phase, Phase::Held(_)))
            .map(|(name, _)| name.clone())
            .collect()
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
        let (root, extra) = (shared.root().to_owned(), shared.rules_text.clone());
        let limit = usize::try_from(limit.min(beep_proto::node::sync::MAX_SKIPPED))?;
        let walked = spawn_blocking(move || {
            let rules = rules::Rules::load(&root, &extra)?;
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
        datasets.extend(self.inner.projects.seen().values().filter_map(|entry| {
            let (owner, _) = beep_proto::node::sync::ignored_folder(&entry.project.name)?;
            (owner == project).then(|| entry.running().cloned())?
        }));

        for result in join_all(datasets.iter().map(|shared| shared.rescan(RESCAN_LIMIT))).await {
            result?;
        }

        self.status_for(project)
            .with_context(|| format!("{project} stopped syncing"))
    }

    /// Starts `project` over in the folder now at its path, with its
    /// ignored-folder datasets: each stops, forgets what it saw, as one
    /// turned off does, and starts again from a fresh scan that fetches what
    /// the folder misses and deletes nothing elsewhere. For a folder that was
    /// replaced, which sync otherwise refuses until the old one is back.
    pub async fn start_over(&self, project: &str) -> Result<()> {
        let project = project.to_owned();
        self.change("start_over", move |syncs| async move {
            syncs.do_start_over(&project).await
        })
        .await
        .unwrap_or_else(|why| bail!(why))
    }

    async fn do_start_over(&self, project: &str) -> Result<()> {
        let state = &self.inner.projects;
        // Listed in any phase: a change that waits may have stopped one.
        let names: Vec<String> = state
            .seen()
            .keys()
            .filter(|name| {
                name.as_str() == project
                    || beep_proto::node::sync::ignored_folder(name)
                        .is_some_and(|(owner, _)| owner == project)
            })
            .cloned()
            .collect();
        let _gates = state.gates(names.clone()).await;
        let running: Vec<String> = names
            .into_iter()
            .filter(|name| {
                state
                    .seen()
                    .get(name)
                    .is_some_and(|entry| entry.phase.is_running())
            })
            .collect();

        if !running.iter().any(|name| name == project) {
            bail!("{project} is not synced on this machine");
        }

        let mut again: Vec<(Project, Source)> = Vec::new();
        let mut stopped = Vec::new();

        for name in &running {
            if let Some(entry) = state.seen().get(name) {
                again.push((entry.project.clone(), entry.from));
            }

            stopped.extend(state.take(name));
        }

        join_all(stopped.into_iter().map(|running| self.stop(running))).await;
        let started = join_all(again.into_iter().map(|(project, from)| async move {
            state.put(&project, from, Phase::Starting);

            match self.run(&project, from).await {
                Ok(running) => {
                    log::info!("{}: started over in {}", project.name, project.folder);
                    self.started(&project, from, running);

                    None
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    log::warn!("{} does not start over: {message}", project.name);
                    state.put(&project, from, Phase::Stopped(message.clone()));

                    Some(message)
                }
            }
        }))
        .await;

        match started.into_iter().flatten().next() {
            Some(message) => bail!(message),
            None => Ok(()),
        }
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

    fn project(&self, name: &str) -> Option<Arc<Shared>> {
        self.inner
            .projects
            .seen()
            .get(name)
            .and_then(Entry::running)
            .cloned()
    }

    pub(crate) fn is_peer(&self, peer: &EndpointId) -> bool {
        self.inner.may_connect(peer)
    }

    /// Whether `peer` may still make requests: it was not removed from the
    /// mesh.
    pub(crate) fn allows(&self, peer: &EndpointId) -> bool {
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
    let reply = answer(&syncs, request)
        .await
        .unwrap_or_else(|e| Reply::Error {
            message: format!("{e:#}"),
        });

    tx.send(&reply).await
}

async fn answer(syncs: &Syncs, request: Request) -> Result<Reply> {
    Ok(match request {
        Request::Configure { projects } if syncs.inner.mesh.get().is_some() => Reply::Configured {
            refused: projects
                .into_iter()
                .map(|project| Refused {
                    project: project.name,
                    message: "this machine takes its projects from its mesh".to_owned(),
                })
                .collect(),
            addr: syncs.addr(),
        },
        Request::Configure { projects } => Reply::Configured {
            refused: syncs.configure(projects).await,
            addr: syncs.addr(),
        },
        Request::Status {} => Reply::Status {
            projects: syncs.status(),
        },
        Request::Skipped { project, limit } => Reply::Skipped {
            skipped: syncs.skipped(&project, limit).await?,
        },
        Request::Resolve {
            project,
            path,
            keep,
        } => {
            syncs.resolve(&project, &path, keep).await?;
            Reply::Resolved {}
        }
        Request::Rescan { project } => Reply::Rescanned {
            status: syncs.rescan(&project).await?,
        },
        Request::HandoffSnapshot { project } => Reply::HandoffSnapshot {
            snapshot: syncs.handoff_snapshot(&project).await?,
        },
        Request::HandoffApply { project, snapshot } => {
            syncs.handoff_apply(&project, snapshot).await?;
            Reply::HandoffApplied {}
        }
        Request::StartOver { project } => {
            syncs.start_over(&project).await?;
            Reply::StartedOver {}
        }
        Request::SyncthingFiles { folder, enabled } => Reply::SyncthingFiles {
            files: syncs.syncthing_files(&folder, enabled).await?,
        },
        other => Reply::Error {
            message: format!("this node cannot do {other:?} yet"),
        },
    })
}

#[cfg(test)]
#[path = "../../tests/files_sync.rs"]
mod tests;
