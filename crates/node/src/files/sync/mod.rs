//! `sync/1` (see `beep_proto::node::sync`): keeps project folders in step
//! with the same project on the user's other machines.

use crate::{
    files::Folders,
    files::syncthing::Syncthing,
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
        Refused, Reply, Request, Skipped,
    },
};
use futures::future::join_all;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_blobs::{Hash, store::fs::FsStore};
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
pub(super) mod stignore;
mod syncthing;
mod watch;

use actor::{Entry, Handle, Op, Phase};
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
    recoveries: Mutex<BTreeMap<String, std::sync::Weak<crate::agents::Agents>>>,
    parts: Parts,
    folders: Folders,
    /// Where the projects' state is kept.
    dir: PathBuf,
    watcher: Watcher,
    /// Every project's state.
    actor: Handle,
    /// Ends the actor with the syncs.
    _actor: AbortOnDropHandle<()>,
    syncthing: Syncthing,
    /// Checks the folders against Syncthing's every so often.
    checking: Mutex<Option<AbortOnDropHandle<()>>>,
    /// The pulls other nodes have open here.
    serving: Arc<AtomicUsize>,
    /// The mesh this node follows, once it is in one: it names the
    /// projects, and its device list says who may sync them.
    mesh: Mutex<Option<Mesh>>,
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
            && self
                .actor
                .seen()
                .values()
                .filter_map(Entry::running)
                .any(|shared| shared.has_peer(peer))
    }

    fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool {
        self.allows(peer)
            && self
                .actor
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
            inner: Arc::new_cyclic(|inner| {
                let (actor, owner) = Handle::start(inner.clone());
                Inner {
                    parts,
                    recoveries: Mutex::default(),
                    folders,
                    dir,
                    watcher,
                    actor,
                    _actor: owner,
                    syncthing,
                    checking: Mutex::default(),
                    serving: Arc::default(),
                    mesh: Mutex::default(),
                }
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

        for (name, entry) in self.inner.actor.seen().iter() {
            if let Some(shared) = entry.running() {
                shared.set_recovery(recoveries.get(name).cloned());
            }
        }
    }

    /// Syncs exactly `projects`, and says which it refused. A project left
    /// out stops; its files stay and its state is forgotten.
    pub async fn configure(&self, projects: Vec<Project>) -> Vec<Refused> {
        self.configure_from(projects, Source::Direct, false).await
    }

    /// Follows `mesh` from now on: its device list says who may sync.
    pub(crate) fn follow(&self, mesh: Mesh) {
        *lock(&self.inner.mesh) = Some(mesh);
    }

    /// Lets go of the mesh `follow` took.
    pub(crate) fn unfollow(&self) {
        *lock(&self.inner.mesh) = None;
        lock(&self.inner.recoveries).clear();

        for shared in self.inner.actor.seen().values().filter_map(Entry::running) {
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
        let op = |done| Op::Configure {
            projects,
            from,
            keep,
            done,
        };

        self.inner.actor.ask(op).await.unwrap_or_else(|why| {
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
        let actor = &self.inner.actor;
        let me = self.inner.parts.endpoint.id();
        let mut refused = Vec::new();
        let mut kept = Vec::new();
        let mut stopping = Vec::new();
        let mut replaced = Vec::new();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        let before = actor.seen();
        let mut forgotten = Vec::new();

        for (name, entry) in before.iter() {
            match entry.phase {
                Phase::Running(_) if !names.contains(&name.as_str()) => {
                    stopping.extend(actor.take_dropped(name).await.map(|running| (name, running)));
                }
                Phase::Held(_) if !names.contains(&name.as_str()) => forgotten.push(name.clone()),
                Phase::Stopped(_) if !names.contains(&name.as_str()) => forgotten.push(name.clone()),
                _ => {}
            }
        }

        actor.remove(forgotten).await;
        let folders: Vec<PathBuf> = projects.iter().map(|p| PathBuf::from(&p.folder)).collect();
        let managed = self.inner.syncthing.check_all(&folders).await;

        for (mut project, managed) in projects.into_iter().zip(managed) {
            project.peers.retain(|peer| peer.id != me);

            if let Some(shared) = actor.keep(&project, from).await {
                log::debug!("sync {}: keeps running, peers and pause set", project.name);
                shared.set_peers(project.peers.clone());
                shared.set_paused(project.paused);
                kept.push(project);
                continue;
            }
            // Stopped before its replacement starts: the same folder named
            // another way has the same key, watches and state file.
            if let Some(old) = actor.take(&project.name).await {
                log::debug!("sync {}: stopping the run it replaces", project.name);
                replaced.push(halt(old).await);
            }
            log::debug!(
                "sync {}: asking Syncthing about {}",
                project.name,
                project.folder
            );
            let was_held = actor.seen().get(&project.name).is_some_and(|entry| {
                matches!(entry.phase, Phase::Held(_)) && entry.project.folder == project.folder
            });

            if let Some(managed) = managed {
                if was_held || keep {
                    log::info!("not syncing {}: {}", project.name, managed.reason());
                    actor
                        .put(&project, from, Phase::Held(managed.reason()))
                        .await;
                    kept.push(project);
                } else {
                    actor.remove(vec![project.name.clone()]).await;
                    refused.push(Refused {
                        project: project.name.clone(),
                        message: managed.reason(),
                    });
                }
                continue;
            }

            log::debug!("sync {}: starting", project.name);
            // One that was held or stopped stays listed as it was until it runs.
            if !actor.seen().get(&project.name).is_some_and(|entry| {
                matches!(entry.phase, Phase::Held(_) | Phase::Stopped(_))
            }) {
                actor.put(&project, from, Phase::Starting).await;
            }

            match self.run(&project, from).await {
                Ok(running) => {
                    actor.start_run(&project, from, running).await;
                    kept.push(project);
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    refused.push(Refused {
                        project: project.name.clone(),
                        message: message.clone(),
                    });

                    if keep {
                        actor.put(&project, from, Phase::Stopped(message)).await;
                        kept.push(project);
                    } else {
                        actor.remove(vec![project.name.clone()]).await;
                    }
                }
            }
        }

        for (name, running) in stopping {
            log::debug!("sync {name}: stopping");
            self.stop(running).await;
            actor.remove(vec![name.clone()]).await;
        }

        for old in replaced {
            self.forget(&old).await;
        }

        // A node that stops refused the loads: its list stays as it was.
        if actor.closing() {
            return refused;
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
        // A node that stops loads nothing: a run that halts still saves its state.
        if self.inner.actor.closing() {
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
        shared.set_recovery(lock(&self.inner.recoveries).get(&project.name).cloned());
        let task = tokio::spawn(project::run(
            shared.clone(),
            self.inner.parts.clone(),
            self.inner.watcher.clone(),
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
        if self.inner.actor.closing()
            || self
                .inner
                .actor
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
        self.inner
            .actor
            .ask(|done| Op::CheckSyncthing { done })
            .await
            .ok();
    }

    async fn do_check_syncthing(&self) {
        let actor = &self.inner.actor;
        let running = self.entries(Phase::is_running);
        let folders: Vec<PathBuf> = running
            .iter()
            .map(|entry| PathBuf::from(&entry.project.folder))
            .collect();
        let managed = self.inner.syncthing.check_all(&folders).await;

        for (entry, managed) in running.into_iter().zip(managed) {
            let Some(managed) = managed else {
                continue;
            };
            let Entry { project, from, .. } = entry;
            log::info!("stopped syncing {}: {}", project.name, managed.reason());

            if let Some(stopped) = actor.take(&project.name).await {
                self.stop(stopped).await;
            }

            actor
                .put(&project, from, Phase::Held(managed.reason()))
                .await;
        }

        let held = self.entries(Phase::is_held);
        let folders: Vec<PathBuf> = held
            .iter()
            .map(|entry| PathBuf::from(&entry.project.folder))
            .collect();
        let managed = self.inner.syncthing.check_all(&folders).await;

        for (entry, managed) in held.into_iter().zip(managed) {
            if managed.is_some() {
                continue;
            }

            match self.run(&entry.project, entry.from).await {
                Ok(running) => {
                    log::info!(
                        "syncing {}: Syncthing let go of it or paused it",
                        entry.project.name
                    );
                    actor
                        .start_run(&entry.project, entry.from, running)
                        .await;
                }
                Err(e) => log::warn!("{} does not sync yet: {e:#}", entry.project.name),
            }
        }
    }

    /// Checks the projects' folders now: a running project whose folder is gone stops (its scan
    /// already refuses to send deletes) and starts again, which makes the folder and fills it
    /// from the peers; a project that did not start is tried again, so a folder that is back,
    /// or whose rules can be followed now, syncs.
    pub async fn check_folders(&self) {
        self.inner.actor.ask(|done| Op::CheckFolders { done }).await.ok();
    }

    async fn do_check_folders(&self) {
        let actor = &self.inner.actor;
        let mut again: Vec<(Project, Source)> = Vec::new();

        for entry in self.entries(Phase::is_running) {
            let folder = PathBuf::from(&entry.project.folder);

            if spawn_blocking(move || folder.exists())
                .await
                .unwrap_or(true)
            {
                continue;
            }

            let Entry { project, from, .. } = entry;
            log::info!("{}: its folder {} is gone", project.name, project.folder);

            if let Some(stopped) = actor.take(&project.name).await {
                self.stop(stopped).await;
            }

            actor.put(&project, from, Phase::Starting).await;
            again.push((project, from));
        }

        again.extend(
            self.entries(Phase::is_stopped)
                .into_iter()
                .map(|entry| (entry.project, entry.from)),
        );

        for (project, from) in again {
            match self.run(&project, from).await {
                Ok(running) => {
                    log::info!("syncing {} again", project.name);
                    actor.start_run(&project, from, running).await;
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    let same = actor.seen().get(&project.name).is_some_and(
                        |entry| matches!(&entry.phase, Phase::Stopped(last) if *last == message),
                    );

                    if !same {
                        log::warn!("not syncing {}: {message}", project.name);
                    }

                    actor.put(&project, from, Phase::Stopped(message)).await;
                }
            }
        }
    }

    /// The projects now in a phase `of` names, by name.
    fn entries(&self, of: fn(&Phase) -> bool) -> Vec<Entry> {
        self.inner
            .actor
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
        self.inner.actor.shutdown().await;
    }

    /// How each project stands. A project in the middle of a change is
    /// still listed.
    pub fn status(&self) -> Vec<ProjectStatus> {
        self.inner
            .actor
            .seen()
            .values()
            .map(Entry::status)
            .collect()
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
            .actor
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
        datasets.extend(self.inner.actor.seen().values().filter_map(|entry| {
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
        self.inner
            .actor
            .ask(|done| Op::StartOver { project, done })
            .await
            .unwrap_or_else(|why| bail!(why))
    }

    async fn do_start_over(&self, project: &str) -> Result<()> {
        let actor = &self.inner.actor;
        let names: Vec<String> = actor
            .seen()
            .iter()
            .filter(|(name, entry)| {
                entry.running().is_some()
                    && (name.as_str() == project
                        || beep_proto::node::sync::ignored_folder(name)
                            .is_some_and(|(owner, _)| owner == project))
            })
            .map(|(name, _)| name.clone())
            .collect();

        if !names.iter().any(|name| name == project) {
            bail!("{project} is not synced on this machine");
        }

        let mut again: Vec<(Project, Source)> = Vec::new();
        let mut stopped = Vec::new();

        for name in &names {
            if let Some(entry) = actor.seen().get(name) {
                again.push((entry.project.clone(), entry.from));
            }

            stopped.extend(actor.take(name).await);
        }

        for running in stopped {
            self.stop(running).await;
        }

        let mut failed = None;

        for (project, from) in again {
            actor.put(&project, from, Phase::Starting).await;

            match self.run(&project, from).await {
                Ok(running) => {
                    log::info!("{}: started over in {}", project.name, project.folder);
                    actor.start_run(&project, from, running).await;
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    log::warn!("{} does not start over: {message}", project.name);
                    failed.get_or_insert_with(|| message.clone());
                    actor.put(&project, from, Phase::Stopped(message)).await;
                }
            }
        }

        match failed {
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
            .actor
            .seen()
            .get(name)
            .and_then(Entry::running)
            .cloned()
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
    let reply = answer(&syncs, request)
        .await
        .unwrap_or_else(|e| Reply::Error {
            message: format!("{e:#}"),
        });

    tx.send(&reply).await
}

async fn answer(syncs: &Syncs, request: Request) -> Result<Reply> {
    Ok(match request {
        Request::Configure { projects } if lock(&syncs.inner.mesh).is_some() => Reply::Configured {
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
