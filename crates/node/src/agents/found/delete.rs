//! Deleting the files of a session in an agent's store (`Stores::delete`),
//! always checked to lie inside the store first.

use super::{Agent, Stores};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

impl Stores {
    /// Deletes the file of `agent`'s session `id`: Codex's rollout or pi's
    /// session file. `false` when there is no such session. Codex's
    /// `state_5.sqlite` and its title index are left as they are. Blocks:
    /// run it with `spawn_blocking`.
    pub(crate) fn delete(&self, agent: Agent, id: &str) -> Result<bool> {
        let root = match agent {
            Agent::Codex => self.codex.as_deref().map(|root| root.join("sessions")),
            Agent::Pi => self.pi.clone(),
            Agent::Claude | Agent::Flsh | Agent::OpenCode => {
                bail!("{} sessions cannot be deleted here", agent.as_str())
            }
        };
        let Some(root) = root else {
            return Ok(false);
        };
        let Some(file) = self.file(Some(agent), id) else {
            return Ok(false);
        };
        remove_file_in(&root, &file)?;
        Ok(true)
    }
}

/// The real path of `path` when it lies strictly inside `root`.
fn inside(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root).context("the store is not readable")?;
    let real = fs::canonicalize(path).context("the session is not readable")?;
    ensure!(
        real != root && real.starts_with(&root),
        "{} is outside the agent's store",
        path.display()
    );
    Ok(real)
}

/// Removes the regular file `file` (not a link) of a session, once it is
/// known to lie inside `root`.
pub(crate) fn remove_file_in(root: &Path, file: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(file).context("the session file is not readable")?;
    ensure!(
        meta.file_type().is_file(),
        "the session is not a plain file"
    );
    let real = inside(root, file)?;
    fs::remove_file(&real).with_context(|| format!("removing {}", real.display()))
}

/// Removes the folder `dir` of a session with what is in it, when it is
/// there, is not a link and lies inside `root`.
pub(crate) fn remove_dir_in(root: &Path, dir: &Path) -> Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_dir() => {
            let real = inside(root, dir)?;
            fs::remove_dir_all(&real).with_context(|| format!("removing {}", real.display()))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("the session folder is not readable"),
    }
}
