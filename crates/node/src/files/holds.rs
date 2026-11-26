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

/// The folders of the sessions started outside Beep that run, by agent and
/// session id.
pub type Externals = BTreeMap<(String, SessionId), PathBuf>;

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
        let external: Vec<PathBuf> = beep_proto::watched::read(&self.0.external, |external| {
            external.values().cloned().collect()
        });
        let busy = tokio::task::spawn_blocking(move || {
            holds.running_under(&folder) || under(&external, &folder)
        })
        .await?;
        ensure!(
            !busy,
            "Local edits block Git sync; close running sessions before automatic recovery."
        );

        Ok(hold)
    }

    /// Whether a session runs in `folder` or below it. Blocks on the file system.
    pub fn running_under(&self, folder: &Path) -> bool {
        let running: Vec<PathBuf> = crate::lock(&self.0.running)
            .iter()
            .map(|(_, path)| path.clone())
            .collect();

        under(&running, folder)
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

/// Whether any of `paths` is in `folder`, as named or with its links resolved.
fn under(paths: &[PathBuf], folder: &Path) -> bool {
    paths.iter().any(|path| {
        path.starts_with(folder)
            || crate::files::paths::canonicalize(path).is_ok_and(|real| real.starts_with(folder))
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
