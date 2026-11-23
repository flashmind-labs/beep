//! The command center: its windows, its app menu, the view each opens on,
//! Home, the views kept for the next window and which agent session each
//! one shows.

use anyhow::Context;
use std::{
    fs,
    path::Path,
    sync::{Mutex, MutexGuard, PoisonError},
};

#[cfg(desktop)]
pub(crate) mod app_menu;
pub(crate) mod home;
pub(crate) mod home_frame;
pub(crate) mod home_sessions;
pub(crate) mod shown;
pub(crate) mod targets;
pub(crate) mod usage;
pub(crate) mod view_cache;
pub(crate) mod window;
#[cfg(desktop)]
pub(crate) mod window_session;

pub(crate) use home::{NeedsYou, needs_you, section};
pub(crate) use shown::AgentsShown;
pub(crate) use targets::{AgentTarget, CenterTarget, Pending, RunAt, open_on};
pub(crate) use view_cache::{cache_view, kept};
pub(crate) use window::{closed, hide_on_close, is_center, open};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Writes `bytes` to `path` through a `.part` sibling so a reader never sees
/// half of it.
pub(crate) fn write_part(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }

    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    fs::write(&partial, bytes).with_context(|| format!("writing {}", path.display()))?;
    fs::rename(&partial, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}
