//! The folder an assistant conversation runs in: its own, under the hidden
//! `.beep/assistant` in the home folder. Each of its threads gets a subfolder
//! named after it, so the conversation's notes sit above what its threads
//! keep and no two threads write over each other.

use beep_proto::{SessionId, node::agent::slug};
use std::{
    io,
    path::{Component, Path, PathBuf},
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

/// The subfolder the thread `id`, called `title`, gets of its parent's
/// folder `parent`, made now, when that is a conversation's or under one.
/// `None` for a chat anywhere else, whose children start in its own folder.
pub(super) fn thread_folder(
    home: &Path,
    parent: &Path,
    title: Option<&str>,
    id: &SessionId,
) -> io::Result<Option<PathBuf>> {
    let root = root(home);

    if parent == root || !parent.starts_with(&root) {
        return Ok(None);
    }

    let name = title.map(slug).filter(|name| !name.is_empty());
    let made = match name {
        Some(name) if !parent.join(&name).exists() => parent.join(name),
        Some(name) => parent.join(format!("{name}-{id}")),
        None => parent.join(id.as_ref()),
    };
    std::fs::create_dir_all(&made)?;
    Ok(Some(made))
}

/// Whether `folder` is a conversation's: a named folder directly under the
/// assistant's folder in `home`, so never that folder itself by `.` or what
/// is above it by `..`.
pub(super) fn holds(home: &Path, folder: &Path) -> bool {
    folder.parent() == Some(root(home).as_path())
        && matches!(folder.components().next_back(), Some(Component::Normal(_)))
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
