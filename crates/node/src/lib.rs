//! `beep-node` runs on a machine so Beep can run work there. Its modules are
//! `agents`, `files`, `link`, `machine` and `run`.

use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(feature = "host")]
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
#[cfg(feature = "host")]
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Marks the items inside as the node's host half: they need the `host`
/// feature, which the phone app leaves out.
macro_rules! host {
    ($($item:item)*) => {
        $(#[cfg(feature = "host")] $item)*
    };
}

pub mod agents;
pub mod events;
pub mod files;
pub mod link;
pub mod peer_stream;

host! {
    pub mod actor;
    pub mod machine;
    mod run;
    mod team;
}

/// Kills every background task and terminal still running, at once. For
/// a host about to exit, such as the Mac app: a process exit drops nothing,
/// so they would outlive it.
#[cfg(feature = "host")]
pub fn kill_tasks() {
    run::tasks::kill_all();
    run::terminal::kill_all();
}

#[cfg(feature = "host")]
pub use run::device::track_phones;
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
#[cfg(feature = "host")]
pub(crate) fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

/// `lock` for writing an `RwLock`.
#[cfg(feature = "host")]
pub(crate) fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(feature = "host")]
pub(crate) fn config_home(
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if cfg!(windows) {
        return dirs::config_dir();
    }

    env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.map(|home| home.join(".config")))
}
