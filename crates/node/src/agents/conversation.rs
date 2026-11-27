//! The folder an assistant conversation runs in: its own, under the hidden
//! `.beep/assistant` in the home folder. Each of its threads gets a subfolder
//! named after it, so the conversation's notes sit above what its threads
//! keep and no two threads write over each other.

use crate::logs::found::HIDDEN;
use beep_proto::{SessionId, node::agent::slug};
use std::{
    io,
    path::{Component, Path, PathBuf},
};

/// How many ids are tried for a folder nobody has yet.
const TRIES: usize = 8;

/// The folder a new conversation gets when it asks for none or for the home
/// folder (`cwd` as `running::folder` reads it): its own, named by one of
/// `id`'s and made now, never one that is there already. `None` for one that
/// names a folder.
pub(super) fn own(
    home: &Path,
    cwd: Option<&str>,
    mut id: impl FnMut() -> SessionId,
) -> io::Result<Option<PathBuf>> {
    if !matches!(cwd.map(str::trim), None | Some("" | "~")) {
        return Ok(None);
    }

    let root = root(home);
    std::fs::create_dir_all(&root)?;
    made_new((0..TRIES).map(|_| root.join(id().as_ref()))).map(Some)
}

/// The subfolder a thread called `title` gets of its parent's folder
/// `parent`, made now, when that is a conversation's or under one: named
/// after it, with one of `id`'s when that name is taken or there is none.
/// Two threads never get the same one, started together or not. `None` for a
/// chat anywhere else, whose children start in its own folder.
pub(super) fn thread_folder(
    home: &Path,
    parent: &Path,
    title: Option<&str>,
    mut id: impl FnMut() -> SessionId,
) -> io::Result<Option<PathBuf>> {
    let root = root(home);

    if parent == root || !parent.starts_with(&root) {
        return Ok(None);
    }

    std::fs::create_dir_all(parent)?;
    let name = title.map(slug).filter(|name| !name.is_empty());
    let plain = name.as_ref().map(|name| parent.join(name));
    let numbered = (0..TRIES).map(|_| match &name {
        Some(name) => parent.join(format!("{name}-{}", id())),
        None => parent.join(id().as_ref()),
    });
    made_new(plain.into_iter().chain(numbered)).map(Some)
}

/// Makes the first of `folders` that is not there yet, in a folder that is,
/// and gives it: whoever makes a folder has it alone.
fn made_new(folders: impl Iterator<Item = PathBuf>) -> io::Result<PathBuf> {
    for folder in folders {
        match std::fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }

    Err(io::Error::new(io::ErrorKind::AlreadyExists, "every folder tried is taken"))
}

/// Whether `folder` is a conversation's: a named folder directly under the
/// assistant's folder in `home`, so never that folder itself by `.` or what
/// is above it by `..`.
pub(super) fn holds(home: &Path, folder: &Path) -> bool {
    folder.parent() == Some(root(home).as_path())
        && matches!(folder.components().next_back(), Some(Component::Normal(_)))
}

/// What the assistant hears about its folder when it runs in `folder`:
/// only in a conversation's own is it told to keep its notes there.
pub(super) fn brief(home: &Path, folder: &Path) -> Option<&'static str> {
    holds(home, folder).then_some(super::brief::OWN_FOLDER)
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

/// The folder under `home` of a session of a chat that moves between
/// machines: `path` names the chat, then the threads down to the session's
/// own, with `/` between. `None` for a path that leaves the chats' folder.
pub(super) fn moved(home: &Path, path: &str) -> Option<PathBuf> {
    let plain = |name: &&str| {
        let mut parts = Path::new(name).components();
        matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None))
    };
    path.split('/')
        .all(|name| plain(&name))
        .then(|| path.split('/').fold(root(home), |folder, name| folder.join(name)))
}

fn root(home: &Path) -> PathBuf {
    home.join(HIDDEN).join("assistant")
}

#[cfg(test)]
#[path = "../tests/agents_conversation.rs"]
mod tests;
