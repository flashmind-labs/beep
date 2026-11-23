//! The one task that owns every project's state. A project is in exactly
//! one `Phase` at any time, and the map is published on a `watch`, so a
//! read in the middle of a change still finds it.
//!
//! Changes that wait (stopping a project, loading it, asking Syncthing) run
//! one at a time as `Op`s in a task the actor owns, not in the task of whoever
//! asked. A caller that goes away leaves its change to finish.

use super::{Inner, Running, Shared, Source, Syncs, halt};
use anyhow::Result;
use beep_proto::node::sync::{Project, ProjectStatus, Refused, State, SyncthingFiles};
use futures::future::join_all;
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Weak},
};
use tokio::{
    sync::{mpsc, oneshot, watch},
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

/// A change that waits, run one at a time, with the reply to its caller.
pub(super) enum Op {
    Configure {
        projects: Vec<Project>,
        from: Source,
        keep: bool,
        done: oneshot::Sender<Vec<Refused>>,
    },
    CheckSyncthing {
        done: oneshot::Sender<()>,
    },
    CheckFolders {
        done: oneshot::Sender<()>,
    },
    StartOver {
        project: String,
        done: oneshot::Sender<Result<()>>,
    },
    SyncthingFiles {
        root: PathBuf,
        enabled: bool,
        done: oneshot::Sender<Result<SyncthingFiles>>,
    },
    HandoffSnapshot {
        project: String,
        done: oneshot::Sender<Result<beep_proto::node::sync::HandoffSnapshot>>,
    },
    HandoffApply {
        project: String,
        snapshot: beep_proto::node::sync::HandoffSnapshot,
        done: oneshot::Sender<Result<()>>,
    },
}

enum Cmd {
    Op(Op),
    /// Sets the project's phase, whatever it was. A run brings its task.
    Put {
        entry: Entry,
        task: Option<AbortOnDropHandle<()>>,
        done: oneshot::Sender<()>,
    },
    /// Moves a running project to `Stopping` and hands its run over to halt.
    Take {
        name: String,
        done: oneshot::Sender<Option<Running>>,
    },
    /// Takes `project` as the new description of the run that has its folder
    /// and rules, and answers that run.
    Keep {
        project: Project,
        from: Source,
        done: oneshot::Sender<Option<Arc<Shared>>>,
    },
    Remove {
        names: Vec<String>,
        done: oneshot::Sender<()>,
    },
    /// Halts every run now, without waiting for a change in progress.
    Shutdown {
        done: oneshot::Sender<()>,
    },
}

/// How the rest of the node talks to the actor.
pub(super) struct Handle {
    cmds: mpsc::Sender<Cmd>,
    seen: watch::Receiver<Seen>,
}

impl Handle {
    /// Starts the actor. It runs its changes as `inner`, which it holds
    /// only for the time of a change.
    pub(super) fn start(inner: Weak<Inner>) -> (Self, AbortOnDropHandle<()>) {
        let (cmds, rx) = mpsc::channel(64);
        let (seen_tx, seen) = watch::channel(Seen::default());
        let actor = Actor {
            map: BTreeMap::new(),
            tasks: BTreeMap::new(),
            seen: seen_tx,
            inner,
            queue: VecDeque::new(),
            ops: JoinSet::new(),
            halts: JoinSet::new(),
            closing: false,
        };
        let task = AbortOnDropHandle::new(tokio::spawn(actor.run(rx)));
        (Self { cmds, seen }, task)
    }

    pub(super) fn seen(&self) -> Seen {
        beep_proto::watched::read(&self.seen, Seen::clone)
    }

    /// Queues a change and waits for its end; `None` when the node is
    /// stopping and left it undone.
    pub(super) async fn ask<T>(&self, op: impl FnOnce(oneshot::Sender<T>) -> Op) -> Option<T> {
        self.call(|done| Cmd::Op(op(done))).await
    }

    async fn call<T>(&self, cmd: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> Option<T> {
        let (done, reply) = oneshot::channel();
        self.cmds.send(cmd(done)).await.ok()?;
        reply.await.ok()
    }

    pub(super) async fn put(&self, project: &Project, from: Source, phase: Phase) {
        let entry = Entry {
            project: project.clone(),
            from,
            phase,
        };
        self.call(|done| Cmd::Put {
            entry,
            task: None,
            done,
        })
        .await;
    }

    pub(super) async fn start_run(&self, project: &Project, from: Source, running: Running) {
        let entry = Entry {
            project: project.clone(),
            from,
            phase: Phase::Running(running.shared),
        };
        self.call(|done| Cmd::Put {
            entry,
            task: Some(running.task),
            done,
        })
        .await;
    }

    pub(super) async fn take(&self, name: &str) -> Option<Running> {
        self.call(|done| Cmd::Take {
            name: name.to_owned(),
            done,
        })
        .await
        .flatten()
    }

    pub(super) async fn keep(&self, project: &Project, from: Source) -> Option<Arc<Shared>> {
        self.call(|done| Cmd::Keep {
            project: project.clone(),
            from,
            done,
        })
        .await
        .flatten()
    }

    pub(super) async fn remove(&self, names: Vec<String>) {
        self.call(|done| Cmd::Remove { names, done }).await;
    }

    pub(super) async fn shutdown(&self) {
        self.call(|done| Cmd::Shutdown { done }).await;
    }
}

struct Actor {
    map: BTreeMap<String, Entry>,
    /// The task of each `Running` project.
    tasks: BTreeMap<String, AbortOnDropHandle<()>>,
    seen: watch::Sender<Seen>,
    inner: Weak<Inner>,
    queue: VecDeque<Op>,
    /// The change in progress, at most one.
    ops: JoinSet<()>,
    /// The runs halting for a shutdown.
    halts: JoinSet<()>,
    closing: bool,
}

impl Actor {
    async fn run(mut self, mut cmds: mpsc::Receiver<Cmd>) {
        loop {
            tokio::select! {
                cmd = cmds.recv() => match cmd {
                    Some(cmd) => self.handle(cmd),
                    None => return,
                },
                Some(ended) = self.ops.join_next() => {
                    if let Err(e) = ended {
                        log::warn!("a change to the synced projects ended early: {e}");
                    }

                    self.settle();
                    self.next();
                }
                Some(_) = self.halts.join_next() => {}
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Op(op) => {
                // A node that stops makes no more changes.
                if !self.closing {
                    self.queue.push_back(op);
                    self.next();
                }
            }
            Cmd::Put { entry, task, done } => {
                let name = entry.project.name.clone();
                self.tasks.remove(&name);

                match (&entry.phase, task) {
                    // A change that ends after the shutdown began halts its
                    // run at once.
                    (Phase::Running(shared), Some(task)) if self.closing => {
                        let running = Running {
                            shared: shared.clone(),
                            task,
                        };
                        self.map.remove(&name);
                        self.halts.spawn(async move {
                            halt(running).await;
                        });
                    }
                    (_, task) => {
                        if let Some(task) = task {
                            self.tasks.insert(name.clone(), task);
                        }

                        self.map.insert(name, entry);
                    }
                }

                self.publish();
                done.send(()).ok();
            }
            Cmd::Take { name, done } => {
                let running = self.take(&name);
                self.publish();
                done.send(running).ok();
            }
            Cmd::Keep {
                project,
                from,
                done,
            } => {
                let kept = self
                    .map
                    .get_mut(&project.name)
                    .filter(|entry| {
                        entry.running().is_some()
                            && entry.project.folder == project.folder
                            && entry.project.rules == project.rules
                    })
                    .and_then(|entry| {
                        entry.project = project;
                        entry.from = from;
                        entry.running().cloned()
                    });
                self.publish();
                done.send(kept).ok();
            }
            Cmd::Remove { names, done } => {
                for name in names {
                    self.map.remove(&name);
                    self.tasks.remove(&name);
                }

                self.publish();
                done.send(()).ok();
            }
            Cmd::Shutdown { done } => {
                self.closing = true;
                self.queue.clear();
                let names: Vec<String> = self
                    .map
                    .iter()
                    .filter(|(_, entry)| entry.running().is_some())
                    .map(|(name, _)| name.clone())
                    .collect();
                let runs: Vec<Running> = names
                    .iter()
                    .filter_map(|name| {
                        let running = self.take(name);
                        self.map.remove(name);
                        running
                    })
                    .collect();
                self.publish();
                // All at once: one busy project holds up no other's save.
                self.halts.spawn(async move {
                    join_all(runs.into_iter().map(halt)).await;
                    done.send(()).ok();
                });
            }
        }
    }

    /// Moves a running project to `Stopping`, handing over its run.
    fn take(&mut self, name: &str) -> Option<Running> {
        let entry = self.map.get_mut(name)?;
        let shared = entry.running()?.clone();
        let task = self.tasks.remove(name)?;
        entry.phase = Phase::Stopping(shared.clone());
        Some(Running { shared, task })
    }

    /// Starts the next queued change when none is in progress.
    fn next(&mut self) {
        if !self.ops.is_empty() {
            return;
        }

        let Some(op) = self.queue.pop_front() else {
            return;
        };
        let Some(inner) = self.inner.upgrade() else {
            self.queue.clear();
            return;
        };
        self.ops.spawn(Syncs { inner }.perform(op));
    }

    /// After a change that ended early, leaves no project half way: a load
    /// that did not finish is a stopped project, which the folder checks try
    /// again, and a halt that did not finish is over.
    fn settle(&mut self) {
        let before = self.map.len();
        self.map
            .retain(|_, entry| !matches!(entry.phase, Phase::Stopping(_)));
        let mut changed = self.map.len() != before;

        for entry in self.map.values_mut() {
            if matches!(entry.phase, Phase::Starting) {
                entry.phase = Phase::Stopped("it did not finish starting".to_owned());
                changed = true;
            }
        }

        if changed {
            self.publish();
        }
    }

    fn publish(&self) {
        self.seen.send_replace(Arc::new(self.map.clone()));
    }
}

impl Syncs {
    async fn perform(self, op: Op) {
        match op {
            Op::Configure {
                projects,
                from,
                keep,
                done,
            } => {
                done.send(self.do_configure(projects, from, keep).await).ok();
            }
            Op::CheckSyncthing { done } => {
                self.do_check_syncthing().await;
                done.send(()).ok();
            }
            Op::CheckFolders { done } => {
                self.do_check_folders().await;
                done.send(()).ok();
            }
            Op::StartOver { project, done } => {
                done.send(self.do_start_over(&project).await).ok();
            }
            Op::SyncthingFiles {
                root,
                enabled,
                done,
            } => {
                done.send(self.do_syncthing_files(&root, enabled).await).ok();
            }
            Op::HandoffSnapshot { project, done } => {
                done.send(self.do_handoff_snapshot(&project).await).ok();
            }
            Op::HandoffApply {
                project,
                snapshot,
                done,
            } => {
                done.send(self.do_handoff_apply(&project, snapshot).await)
                    .ok();
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_actor.rs"]
mod tests;
