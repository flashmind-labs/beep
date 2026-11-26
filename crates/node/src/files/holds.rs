//! Checkouts that Git sync holds while it preserves and restores local
//! edits, and the folders sessions run in. The agents hold a checkout here
//! for sync, register their sessions' folders and check a start against
//! the holds.

use anyhow::{Result, ensure};
use std::{
    collections::{BTreeSet, HashSet},
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

/// The checkouts whose local edits wait for Git recovery, published by the
/// sync that wants it. Whoever may let it run follows this, checks the
/// checkout is idle and answers each root through `Syncs::answer_recovery`.
#[derive(Clone, Default)]
pub struct Recoveries(Arc<watch::Sender<BTreeSet<PathBuf>>>);

impl Recoveries {
    /// The roots waiting for an answer, changing on every ask.
    pub fn subscribe(&self) -> watch::Receiver<BTreeSet<PathBuf>> {
        self.0.subscribe()
    }

    pub(crate) fn ask(&self, root: &Path) {
        self.0.send_modify(|roots| {
            roots.insert(root.to_owned());
        });
    }

    pub(crate) fn answered(&self, root: &Path) {
        self.0.send_if_modified(|roots| roots.remove(root));
    }
}

/// The held checkouts and the folders in use. Clones share them.
#[derive(Clone, Default)]
pub struct RepositoryHolds(Arc<Inner>);

#[derive(Default)]
struct Inner {
    held: Mutex<HashSet<PathBuf>>,
    running: Mutex<Vec<(u64, PathBuf)>>,
    next: AtomicU64,
}

/// A checkout held; released when dropped.
pub struct RepositoryHold {
    holds: RepositoryHolds,
    folder: PathBuf,
}

/// A folder a session runs in or starts in; released when dropped.
pub struct Running {
    holds: RepositoryHolds,
    id: u64,
}

impl RepositoryHolds {
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
    /// refused while another hold has it. A start registers its folder
    /// before it checks the holds, so a start this misses finds the folder
    /// held; whether a session already runs in it is for the caller to ask.
    pub async fn hold(&self, folder: &Path) -> Result<RepositoryHold> {
        let folder = folder.to_owned();
        let folder =
            tokio::task::spawn_blocking(move || crate::files::paths::canonicalize(&folder))
                .await??;
        let mut held = crate::lock(&self.0.held);
        ensure!(
            !held.contains(&folder),
            "Git recovery already runs in this checkout."
        );
        held.insert(folder.clone());

        Ok(RepositoryHold {
            holds: self.clone(),
            folder,
        })
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
