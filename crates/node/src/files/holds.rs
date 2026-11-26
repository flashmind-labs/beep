//! Checkouts that Git sync holds while it preserves and restores local
//! edits, and the folders sessions run in. Sync reserves a checkout here;
//! the agents register their sessions' folders and check a start against
//! the holds.

use anyhow::{Result, ensure};
use beep_proto::SessionId;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::watch;

/// The most sessions of one folder a read of the agents' stores counts. A
/// folder with that many or more cannot be known idle.
pub const MAX_IN_FOLDERS: usize = 100;

/// A session started outside Beep, as the agents' last read of the stores
/// saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outside {
    pub cwd: PathBuf,
    /// Anything but known to be stopped: an unknown presence is not idle.
    pub busy: bool,
}

/// What the agents know of the sessions started outside Beep, by agent and
/// session id, published for Git sync to read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Externals {
    /// Whether the agents have read their stores. Until then no presence is
    /// established, and no checkout is known to be idle.
    pub read: bool,
    pub sessions: BTreeMap<(String, SessionId), Outside>,
}

/// The held checkouts and the folders in use. Clones share them.
#[derive(Clone)]
pub struct RepositoryHolds(Arc<Inner>);

struct Inner {
    /// The folders of running sessions started outside Beep, published by
    /// the agents.
    external: watch::Receiver<Arc<Externals>>,
    held: Mutex<HashSet<PathBuf>>,
    running: Mutex<Vec<(u64, PathBuf)>>,
    next: AtomicU64,
}

/// A checkout held; released when dropped.
pub(crate) struct RepositoryHold {
    holds: RepositoryHolds,
    folder: PathBuf,
}

/// A folder a session runs in or starts in; released when dropped.
pub struct Running {
    holds: RepositoryHolds,
    id: u64,
}

impl RepositoryHolds {
    pub fn new(external: watch::Receiver<Arc<Externals>>) -> Self {
        Self(Arc::new(Inner {
            external,
            held: Mutex::default(),
            running: Mutex::default(),
            next: AtomicU64::default(),
        }))
    }

    /// Marks `cwd` as the folder of a start or session until the guard drops.
    pub fn running(&self, cwd: PathBuf) -> Running {
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        crate::lock(&self.0.running).push((id, cwd));

        Running {
            holds: self.clone(),
            id,
        }
    }

    /// Holds the checkout `folder` while sync preserves its local edits,
    /// refused while a session runs in it or another hold has it.
    pub(crate) async fn reserve(&self, folder: &Path) -> Result<RepositoryHold> {
        let folder = folder.to_owned();
        let folder =
            tokio::task::spawn_blocking(move || crate::files::paths::canonicalize(&folder))
                .await??;
        {
            let mut held = crate::lock(&self.0.held);
            ensure!(
                !held.contains(&folder),
                "Git recovery already runs in this checkout."
            );
            held.insert(folder.clone());
        }
        // A start registers its folder before it checks the holds, so a start
        // this misses finds the folder held.
        let hold = RepositoryHold {
            holds: self.clone(),
            folder: folder.clone(),
        };
        let holds = self.clone();
        let (read, outside) = beep_proto::watched::read(&self.0.external, |external| {
            let outside: Vec<(PathBuf, bool)> = external
                .sessions
                .values()
                .map(|session| (session.cwd.clone(), session.busy))
                .collect();

            (external.read, outside)
        });
        let (here, count, outside_busy) = tokio::task::spawn_blocking(move || {
            let (count, outside_busy) = outside_under(&outside, &folder);

            (holds.running_under(&folder), count, outside_busy)
        })
        .await?;
        ensure!(
            !here,
            "Local edits block Git sync; close running sessions before automatic recovery."
        );
        // A presence that cannot be established is not evidence of an idle checkout.
        ensure!(
            read && count < MAX_IN_FOLDERS && !outside_busy,
            "Local edits block Git sync; an external session is running or its presence is unknown."
        );

        Ok(hold)
    }

    /// Whether a session runs in `folder` or below it. Blocks on the file system.
    pub fn running_under(&self, folder: &Path) -> bool {
        let running: Vec<PathBuf> = crate::lock(&self.0.running)
            .iter()
            .map(|(_, path)| path.clone())
            .collect();

        running.iter().any(|path| is_under(path, folder))
    }

    /// Refuses a session in `cwd` while sync preserves the local edits of
    /// its checkout.
    pub fn check_start(&self, cwd: &Path) -> Result<()> {
        let real = crate::files::paths::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
        ensure!(
            !crate::lock(&self.0.held)
                .iter()
                .any(|root| real.starts_with(root)),
            "Git sync is preserving local edits in this checkout; try starting the session again shortly."
        );

        Ok(())
    }
}

/// Whether `path` is in `folder`, as named or with its links resolved.
fn is_under(path: &Path, folder: &Path) -> bool {
    path.starts_with(folder)
        || crate::files::paths::canonicalize(path).is_ok_and(|real| real.starts_with(folder))
}

/// How many of the outside sessions `paths` run in `folder` or below, and
/// whether one of them may run.
fn outside_under(paths: &[(PathBuf, bool)], folder: &Path) -> (usize, bool) {
    paths
        .iter()
        .filter(|(path, _)| is_under(path, folder))
        .fold((0, false), |(count, running), (_, busy)| {
            (count + 1, running || *busy)
        })
}

impl Drop for RepositoryHold {
    fn drop(&mut self) {
        crate::lock(&self.holds.0.held).remove(&self.folder);
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        crate::lock(&self.holds.0.running).retain(|(id, _)| *id != self.id);
    }
}
