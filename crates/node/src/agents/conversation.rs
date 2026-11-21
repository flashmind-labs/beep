//! The folder an assistant conversation runs in: its own, under the hidden
//! `.beep/assistant` in the home folder, shared with its threads.

use beep_proto::SessionId;
use std::{
    io,
    path::{Path, PathBuf},
};

/// The hidden folder in the home folder that holds what Beep keeps there.
pub(super) const HIDDEN: &str = ".beep";

/// The folder of the conversation `id` under `home`.
pub(super) fn folder(home: &Path, id: &SessionId) -> PathBuf {
    root(home).join(id.as_ref())
}

/// The folder a new conversation `id` gets when it asks for none or for the
/// home folder (`cwd` as `running::folder` reads it): its own, made now.
/// `None` for one that names a folder.
pub(super) fn own(home: &Path, cwd: Option<&str>, id: &SessionId) -> io::Result<Option<PathBuf>> {
    if !matches!(cwd.map(str::trim), None | Some("" | "~")) {
        return Ok(None);
    }

    let made = folder(home, id);
    std::fs::create_dir_all(&made)?;
    Ok(Some(made))
}

/// Whether `folder` is a conversation's: directly under the assistant's
/// folder in `home`.
pub(super) fn holds(home: &Path, folder: &Path) -> bool {
    folder.parent() == Some(root(home).as_path())
}

/// Removes a conversation's `folder` with what it holds. Any other folder
/// is left alone, and one that is gone already is no failure.
pub(super) fn remove(home: &Path, folder: &Path) -> io::Result<()> {
    if !holds(home, folder) {
        return Ok(());
    }

    match std::fs::remove_dir_all(folder) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn root(home: &Path) -> PathBuf {
    home.join(HIDDEN).join("assistant")
}

#[cfg(test)]
#[path = "../tests/agents_conversation.rs"]
mod tests;
