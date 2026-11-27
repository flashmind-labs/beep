//! Whether the machine's Syncthing keeps a folder in step
//! (`Syncthing::check`). Beep's own sync leaves such a folder alone, since
//! two tools syncing one folder fight over every change. A folder paused in
//! Syncthing is not kept in step, so Beep syncs it until Syncthing resumes it.

use crate::machine::env::syncthing::{
    CHANGE_WAIT, Daemon, Gui, TIMEOUT, fetch, get, query_value, real, request, resolve,
};
use anyhow::{Context, Result};
use beep_proto::node::SyncthingFolder;
use futures::{StreamExt, stream::FuturesUnordered};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    task::spawn_blocking,
    time::{sleep, timeout},
};

pub(crate) use crate::machine::env::syncthing::expand;

mod project;
pub(in crate::files) mod stignore;

/// What Syncthing leaves in the root of each folder it syncs.
const MARKER: &str = ".stfolder";

/// The Syncthing folder that keeps a folder in step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Managed {
    /// Its folder ID, when the API named it.
    pub id: Option<String>,
    /// What to call it: its label or ID, or the marked folder's name.
    pub label: String,
    /// The Syncthing folder itself, the one given or one above it.
    pub root: PathBuf,
}

impl Managed {
    /// Why Beep does not sync the folder.
    pub fn reason(&self) -> String {
        format!("Already synced by Syncthing ({})", self.label)
    }
}

/// The machine's Syncthing, as far as it can be found.
#[derive(Clone)]
pub struct Syncthing {
    inner: Arc<Daemon>,
}

impl Syncthing {
    /// The GUI to ask, from `config.xml`.
    async fn gui(&self) -> Result<Gui> {
        self.inner.gui().await
    }

    /// The Syncthing of the user running the node, in its usual places.
    pub fn local() -> Self {
        Self {
            inner: Arc::new(Daemon::local()),
        }
    }

    /// A Syncthing whose `config.xml` is in the first of `homes` that has
    /// one, for a user whose home is `home`.
    pub fn at(homes: Vec<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Daemon::at(homes, home)),
        }
    }

    /// The Syncthing folder that keeps `folder` in step, if any. Asks
    /// Syncthing's REST API, with the address and key from its `config.xml`; when it does not
    /// answer, a `.stfolder` marker in the folder or above it, up to home,
    /// counts. Never fails: what cannot be read counts as not managed.
    pub async fn check(&self, folder: &Path) -> Option<Managed> {
        let listed = self.inner.folders().await;
        self.decide(folder, listed).await
    }

    /// `check` for many folders at once: Syncthing is asked once and every
    /// folder is decided at the same time. The answers are in `folders`' order.
    pub async fn check_all(&self, folders: &[PathBuf]) -> Vec<Option<Managed>> {
        let listed = self.inner.folders().await;
        let mut deciding: FuturesUnordered<_> = folders
            .iter()
            .enumerate()
            .map(|(at, folder)| {
                let listed = listed.clone();
                async move { (at, self.decide(folder, listed).await) }
            })
            .collect();
        let mut decided = vec![None; folders.len()];

        while let Some((at, managed)) = deciding.next().await {
            decided[at] = managed;
        }

        decided
    }

    /// Waits until Syncthing pauses, resumes or reconfigures a folder, or
    /// `CHANGE_WAIT` has passed. `since` is the last event seen, 0 at first.
    /// When its events cannot be read, waits out `CHANGE_WAIT`.
    pub(crate) async fn changed(&self, since: &mut u64) {
        match self.inner.next_event(*since).await {
            Ok(last) => *since = last.unwrap_or(*since),
            Err(e) => {
                log::debug!("Syncthing's events did not read: {e:#}");
                // Its event numbers start over when it restarts.
                *since = 0;
                sleep(CHANGE_WAIT).await;
            }
        }
    }

    /// Every folder Syncthing keeps in step, as its API lists them less the
    /// paused ones; empty when it does not answer.
    pub async fn folders(&self) -> Vec<SyncthingFolder> {
        self.inner.folders().await.unwrap_or_default()
    }

    async fn decide(&self, folder: &Path, listed: Option<Vec<SyncthingFolder>>) -> Option<Managed> {
        let folder = folder.to_owned();
        let home = self.inner.home().map(Path::to_owned);
        let decided = spawn_blocking(move || {
            let folder = real(&folder);
            let home = home.as_deref().map(real);
            let managed = match listed {
                Some(listed) => holding(&listed, &folder),
                None => marked(&folder, home.as_deref()),
            };

            if let Some(managed) = &managed
                && let Err(e) = protect_checkout(&managed.root)
            {
                log::warn!("Syncthing ignore rules could not be updated: {e:#}");
            }
            // An unreadable ignore file leaves ownership with Syncthing.
            managed.filter(|managed| {
                !stignore::project_ignored(&folder, &managed.root)
                    .unwrap_or(false)
            })
        })
        .await;

        match decided {
            Ok(managed) => managed,
            Err(e) => {
                log::warn!("the Syncthing check did not finish: {e}");
                None
            }
        }
    }

}

/// These rules go first because Syncthing uses the first matching rule.
const LOCAL_GIT: &str =
    "// Beep: Git metadata and worktrees stay on this machine.\n.git\n.worktrees\n";

fn protect_checkout(root: &Path) -> Result<()> {
    use std::io::{ErrorKind, Write};

    let path = root.join(".stignore");
    let old = match fs::read_to_string(&path) {
        Ok(old) => old,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context("reading Syncthing ignore rules"),
    };
    let text = old.trim_start_matches('\u{feff}');

    if text.starts_with(LOCAL_GIT) {
        return Ok(());
    }

    let mut temp = tempfile::NamedTempFile::new_in(root)?;

    if let Ok(meta) = fs::metadata(&path) {
        temp.as_file().set_permissions(meta.permissions())?;
    }

    temp.write_all(LOCAL_GIT.as_bytes())?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(&path)
        .context("saving Syncthing ignore rules")?;
    Ok(())
}

/// The innermost of `listed` that is `folder` or holds it.
fn holding(listed: &[SyncthingFolder], folder: &Path) -> Option<Managed> {
    listed
        .iter()
        .filter(|synced| folder.starts_with(Path::new(&synced.path)))
        .max_by_key(|synced| Path::new(&synced.path).components().count())
        .map(|synced| Managed {
            id: Some(synced.id.clone()),
            label: synced.name().to_owned(),
            root: PathBuf::from(&synced.path),
        })
}

/// The folder at or above `folder`, up to `home`, that holds a marker.
fn marked(folder: &Path, home: Option<&Path>) -> Option<Managed> {
    for dir in folder.ancestors() {
        if dir.join(MARKER).exists() {
            let label = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.to_string_lossy().into_owned());
            return Some(Managed {
                id: None,
                label,
                root: dir.to_owned(),
            });
        }

        if home == Some(dir) {
            break;
        }
    }

    None
}

#[cfg(test)]
#[path = "../../tests/files_syncthing.rs"]
mod tests;
