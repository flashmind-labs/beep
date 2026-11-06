//! A node with no service to restart it, in a container or a foreground
//! `beep-node run`, starts its new binary itself: an update asks for it,
//! the node stops as on a signal, and `main` execs the binary in its place,
//! with the same arguments and environment.

use std::{
    path::{Path, PathBuf},
    sync::{LazyLock, OnceLock},
};
use tokio::sync::Notify;

/// The binary to run in place of this one, once asked.
static NEXT: OnceLock<PathBuf> = OnceLock::new();
static ASKED: LazyLock<Notify> = LazyLock::new(Notify::new);

/// Asks the node to stop and run `exe` in its place. Only the first ask
/// counts.
pub fn ask(exe: PathBuf) {
    if NEXT.set(exe).is_ok() {
        // Kept until the node waits for it.
        ASKED.notify_one();
    }
}

/// Finishes once `ask` was called.
pub async fn asked() {
    ASKED.notified().await;
}

/// The binary asked for, if any.
pub fn next() -> Option<&'static Path> {
    NEXT.get().map(PathBuf::as_path)
}

/// Replaces this process with `exe`, given this process's arguments. Only
/// returns when that failed.
#[cfg(unix)]
pub fn exec(exe: &Path) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    log::info!("starting {} in place of this node", exe.display());
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .exec()
}
