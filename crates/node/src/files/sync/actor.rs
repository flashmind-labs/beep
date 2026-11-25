//! Every project's state. A project is in exactly one `Phase` at any time,
//! behind one private lock, and the map is published on a `watch`, so a read
//! in the middle of a change still finds it.
//!
//! Changes that wait (stopping a project, loading it, asking Syncthing) run
//! as tasks `Syncs::change` owns, not in the task of whoever asked: a caller
//! that goes away leaves its change to finish. Changes to one project never
//! overlap (each takes the project's `Gate`); changes to different projects
//! run together.

use crate::{AsyncMutex, actor, lock};
use super::{FAILED, Running, STOPPING, Shared, Source, Syncs, halt};
use beep_proto::node::sync::{Project, ProjectStatus, State};
use futures::future::join_all;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    sync::{OwnedMutexGuard, oneshot, watch},
    task::JoinSet,
};
use tokio_util::task::AbortOnDropHandle;

/// Every project, as the actor last published them.
pub(super) type Seen = Arc<BTreeMap<String, Entry>>;

/// Where one project stands.
#[derive(Clone)]
pub(super) enum Phase {
    Running(Arc<Shared>),
    /// Halting: it saves its state and takes no peers.
    Stopping(Arc<Shared>),
    /// Loading.
    Starting,
    /// Syncthing already keeps its folder in step: kept on the list, not
    /// synced, until Syncthing lets go. Holds the reason.
    Held(String),
    /// The mesh or the saved list names it, but it does not run here, and
    /// why: tried again with the folder checks, so a folder that comes back
    /// starts syncing.
    Stopped(String),
}

impl Phase {
    pub(super) fn is_running(&self) -> bool {
        matches!(self, Self::Running(_))
    }

    pub(super) fn is_held(&self) -> bool {
        matches!(self, Self::Held(_))
    }

    pub(super) fn is_stopped(&self) -> bool {
        matches!(self, Self::Stopped(_))
    }
}

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) project: Project,
    pub(super) from: Source,
    pub(super) phase: Phase,
}

impl Entry {
    /// The run that syncs now, not one halting.
    pub(super) fn running(&self) -> Option<&Arc<Shared>> {
        match &self.phase {
            Phase::Running(shared) => Some(shared),
            _ => None,
        }
    }

    pub(super) fn status(&self) -> ProjectStatus {
        let failed = |error: &String| ProjectStatus {
            name: self.project.name.clone(),
            folder: self.project.folder.clone(),
            state: State::Failed,
            error: Some(error.clone()),
            ..ProjectStatus::default()
        };

        match &self.phase {
            Phase::Running(shared) | Phase::Stopping(shared) => shared.status(),
            Phase::Held(reason) | Phase::Stopped(reason) => failed(reason),
            Phase::Starting => ProjectStatus {
                name: self.project.name.clone(),
                folder: self.project.folder.clone(),
                ..ProjectStatus::default()
            },
        }
    }
}

/// The project map and the task of each `Running` project.
#[derive(Default)]
struct Map {
    entries: BTreeMap<String, Entry>,
    tasks: BTreeMap<String, AbortOnDropHandle<()>>,
}

impl Map {
    /// Moves a running project to `Stopping`, handing over its run.
    fn take(&mut self, name: &str) -> Option<Running> {
        let entry = self.entries.get_mut(name)?;
        let shared = entry.running()?.clone();
        let task = self.tasks.remove(name)?;
        entry.phase = Phase::Stopping(shared.clone());
        Some(Running { shared, task })
    }
}

/// Every project's state, and who may change it now.
pub(super) struct Projects {
    map: Mutex<Map>,
    seen: watch::Sender<Seen>,
    /// Set once the shutdown began. A change in progress starts and
    /// forgets nothing from then on.
    closing: AtomicBool,
    /// One per project (and per Syncthing folder): the change that holds it
    /// is the only one on that project.
    gates: Mutex<BTreeMap<String, Arc<AsyncMutex<()>>>>,
    /// Held by a change that sets the whole list, which names exactly the
    /// projects that sync: two of them never interleave.
    pub(super) list: AsyncMutex<()>,
    /// The changes in progress.
    ops: Mutex<JoinSet<()>>,
    /// The runs halting for a shutdown.
    halts: Mutex<JoinSet<()>>,
}

impl Default for Projects {
    fn default() -> Self {
        Self {
            map: Mutex::default(),
            seen: watch::Sender::new(Seen::default()),
            closing: AtomicBool::new(false),
            gates: Mutex::default(),
            list: AsyncMutex::new(()),
            ops: Mutex::default(),
            halts: Mutex::default(),
        }
    }
}

impl Projects {
    pub(super) fn closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    pub(super) fn seen(&self) -> Seen {
        beep_proto::watched::read_sent(&self.seen, Seen::clone)
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<Seen> {
        self.seen.subscribe()
    }

    fn publish(&self, map: &Map) {
        self.seen.send_replace(Arc::new(map.entries.clone()));
    }

    /// Whether `project` is listed in a phase `of` names, with its folder.
    pub(super) fn is(&self, project: &Project, of: fn(&Phase) -> bool) -> bool {
        self.seen()
            .get(&project.name)
            .is_some_and(|entry| of(&entry.phase) && entry.project.folder == project.folder)
    }

    /// Waits for the change now on `name`, if any, and takes its place.
    pub(super) async fn gate(&self, name: &str) -> Gate<'_> {
        let gate = {
            let mut gates = lock(&self.gates);
            gates.retain(|_, gate| Arc::strong_count(gate) > 1);
            gates.entry(name.to_owned()).or_default().clone()
        };

        Gate {
            projects: self,
            name: name.to_owned(),
            _held: gate.lock_owned().await,
        }
    }

    /// The gates of `names`, taken in the order given, which is the same
    /// for every change that takes several.
    pub(super) async fn gates(&self, names: impl IntoIterator<Item = String>) -> Vec<Gate<'_>> {
        let mut held = Vec::new();

        for name in names {
            held.push(self.gate(&name).await);
        }

        held
    }

    /// Sets the project's phase, whatever it was. A run brings its task.
    /// Refused over a project that runs: its task would be dropped unsaved.
    pub(super) fn put(&self, project: &Project, from: Source, phase: Phase) {
        let entry = Entry {
            project: project.clone(),
            from,
            phase,
        };
        self.put_entry(entry, None);
    }

    pub(super) fn start_run(&self, project: &Project, from: Source, running: Running) {
        let entry = Entry {
            project: project.clone(),
            from,
            phase: Phase::Running(running.shared),
        };
        self.put_entry(entry, Some(running.task));
    }

    fn put_entry(&self, entry: Entry, task: Option<AbortOnDropHandle<()>>) {
        let name = entry.project.name.clone();
        let mut map = lock(&self.map);
        let accepted = !map.tasks.contains_key(&name);

        if !accepted {
            // Nothing is set over a run that has its task.
            log::warn!("sync {name}: it runs already, so it is not set again");
        } else if self.closing() {
            // A change that ends after the shutdown began leaves nothing
            // listed, and halts a run it started.
            if let (Phase::Running(shared), Some(task)) = (entry.phase, task) {
                let running = Running { shared, task };
                lock(&self.halts).spawn(async move {
                    halt(running).await;
                });
            }
        } else {
            if let Some(task) = task {
                map.tasks.insert(name.clone(), task);
            }

            map.entries.insert(name, entry);
        }

        self.publish(&map);
    }

    /// Hands a running project's run over to halt. It stays listed as
    /// `Stopping`.
    pub(super) fn take(&self, name: &str) -> Option<Running> {
        let mut map = lock(&self.map);
        let running = map.take(name);
        self.publish(&map);

        running
    }

    /// `take` for a project that left the list: it is not listed meanwhile.
    pub(super) fn take_dropped(&self, name: &str) -> Option<Running> {
        let mut map = lock(&self.map);
        let running = map.take(name);

        if running.is_some() {
            map.entries.remove(name);
        }

        self.publish(&map);

        running
    }

    /// Takes `project` as the new description of the run that has its folder
    /// and rules, and answers that run.
    pub(super) fn keep(&self, project: &Project, from: Source) -> Option<Arc<Shared>> {
        let mut map = lock(&self.map);
        let kept = map
            .entries
            .get_mut(&project.name)
            .filter(|entry| {
                entry.running().is_some()
                    && entry.project.folder == project.folder
                    && entry.project.rules == project.rules
            })
            .and_then(|entry| {
                entry.project = project.clone();
                entry.from = from;
                entry.running().cloned()
            });
        self.publish(&map);

        kept
    }

    pub(super) fn remove(&self, names: &[String]) {
        let mut map = lock(&self.map);

        for name in names {
            map.entries.remove(name);
            map.tasks.remove(name);
        }

        self.publish(&map);
    }

    /// After a change that ended early, leaves no project half way: one
    /// that did not finish starting or stopping is a stopped project, which
    /// the folder checks try again.
    fn settle(&self, name: &str) {
        let mut map = lock(&self.map);
        let Some(entry) = map.entries.get_mut(name) else {
            return;
        };
        let why = match entry.phase {
            Phase::Starting => "it did not finish starting",
            Phase::Stopping(_) => "it did not finish stopping",
            _ => return,
        };
        entry.phase = Phase::Stopped(why.to_owned());
        self.publish(&map);
    }

    /// Halts every run now, without waiting for a change in progress.
    pub(super) async fn shutdown(&self) {
        let runs = {
            let mut map = lock(&self.map);
            self.closing.store(true, Ordering::Release);
            let names: Vec<String> = map
                .entries
                .iter()
                .filter(|(_, entry)| entry.running().is_some())
                .map(|(name, _)| name.clone())
                .collect();
            let runs: Vec<Running> = names.iter().filter_map(|name| map.take(name)).collect();
            // Nothing is listed once the node stops, whatever phase it was in.
            map.entries.clear();
            self.publish(&map);

            runs
        };
        let (done, halted) = oneshot::channel();
        // All at once: one busy project holds up no other's save.
        lock(&self.halts).spawn(async move {
            join_all(runs.into_iter().map(halt)).await;
            done.send(()).ok();
        });
        halted.await.ok();
    }
}

/// The place of the one change on a project. A change that ends early
/// (a panic, or its task dropped) leaves the project settled, not half way.
pub(super) struct Gate<'a> {
    projects: &'a Projects,
    name: String,
    _held: OwnedMutexGuard<()>,
}

impl Drop for Gate<'_> {
    fn drop(&mut self) {
        self.projects.settle(&self.name);
    }
}

impl Syncs {
    /// Runs a change as a task of the syncs and waits for its end; says why
    /// when it did not end: the node is stopping, or the change failed.
    pub(super) async fn change<T, Fut>(
        &self,
        label: &'static str,
        change: impl FnOnce(Syncs) -> Fut,
    ) -> Result<T, &'static str>
    where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let projects = &self.inner.projects;

        // A node that stops makes no more changes.
        if projects.closing() {
            return Err(STOPPING);
        }

        let (done, reply) = oneshot::channel();
        let work = actor::job("sync", label, change(self.clone()));
        {
            let mut ops = lock(&projects.ops);

            while let Some(ended) = ops.try_join_next() {
                if let Err(e) = ended {
                    log::warn!("a change to the synced projects ended early: {e}");
                }
            }

            ops.spawn(async move {
                done.send(work.await).ok();
            });
        }

        reply
            .await
            .map_err(|_| if projects.closing() { STOPPING } else { FAILED })
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_actor.rs"]
mod tests;
