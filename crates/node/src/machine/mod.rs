//! The machine itself: what it has (`env`, `stats`), installing and running
//! the node (`setup`, `service`, `update`, `provision`), and repository
//! webhooks (`webhook`), and the watch on the CI runs of pinned projects (`ci`).

pub(crate) mod ci;
pub mod env;
pub mod provision;
pub mod service;
pub mod setup;
pub(crate) mod stats;
#[cfg(feature = "cli")]
pub mod update;
pub mod webhook;

/// The most open files the node asks for; the common soft limits, 256 on a
/// Mac and 1024 on Linux, are low for a node that syncs folders.
#[cfg(unix)]
const OPEN_FILES: u64 = 10_240;

/// Raises this process's soft limit on open files toward `OPEN_FILES`, up to
/// its hard limit. Failing leaves the limit as it was.
#[cfg(unix)]
pub fn raise_open_files() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    let limit = getrlimit(Resource::Nofile);
    let wanted = limit
        .maximum
        .map_or(OPEN_FILES, |hard| hard.min(OPEN_FILES));
    if limit.current.is_some_and(|soft| soft < wanted) {
        let raised = Rlimit {
            current: Some(wanted),
            maximum: limit.maximum,
        };
        if let Err(e) = setrlimit(Resource::Nofile, raised) {
            log::warn!("could not raise the open files limit to {wanted}: {e}");
        }
    }
}
