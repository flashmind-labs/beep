//! `beep-node` runs on a machine so Beep can run work there. Its modules are
//! `agents`, `files`, `link`, `machine` and `run`.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub mod agents;
pub mod files;
pub mod link;
pub mod machine;
mod run;

/// Kills every background task and terminal still running, at once. For
/// a host about to exit, such as the Mac app: a process exit drops nothing,
/// so they would outlive it.
pub fn kill_tasks() {
    run::tasks::kill_all();
    run::terminal::kill_all();
}

#[cfg(all(unix, feature = "cli"))]
pub use run::terminal::host::in_process as terminal_host_in_process;
/// Runs `beep-node pty-host`, which keeps the node's terminals.
#[cfg(all(unix, feature = "cli"))]
pub use run::terminal::host::run as run_terminal_host;

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

/// Locks `mutex`, carrying on with what a holder that panicked left.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `lock` for reading an `RwLock`.
pub(crate) fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

/// `lock` for writing an `RwLock`.
pub(crate) fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}
