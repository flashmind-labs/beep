use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;

use log::warn;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::install::{InstallSwap, ManageError, PendingInstall, clean_leftovers};
use crate::lock::locked;
use crate::manifest::{Manifest, Permission, PluginId};

/// The file in the plugins folder that holds each plugin's record. Names that
/// start with a dot are never plugins.
pub const STATE_FILE: &str = ".plugins.json";

const STATE_TMP: &str = ".plugins.json.tmp";
const STATE_VERSION: u32 = 1;

/// Which plugins run without a record of the user's approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// A plugin runs only when its record says it is enabled and approved. A
    /// folder with no record waits for approval.
    Managed,
    /// A development folder: a plugin with no record is enabled and approved.
    Dev,
}

#[derive(Debug, Error)]
#[error("invalid commit {0:?}: use 7 to 64 hex digits")]
pub struct InvalidCommit(String);

/// A Git commit id as hex text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Commit(String);

impl Commit {
    /// The first seven digits, as people write a commit.
    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(7)]
    }
}

impl FromStr for Commit {
    type Err = InvalidCommit;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let valid = (7..=64).contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit());
        if valid {
            Ok(Self(text.to_ascii_lowercase()))
        } else {
            Err(InvalidCommit(text.to_owned()))
        }
    }
}

impl TryFrom<String> for Commit {
    type Error = InvalidCommit;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Commit> for String {
    fn from(commit: Commit) -> Self {
        commit.0
    }
}

impl fmt::Display for Commit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a Git plugin came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOrigin {
    pub url: String,
    /// The branch, tag or commit asked for; none means the default branch.
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
    /// The folder of the repository that holds the plugin; none means the root.
    #[serde(default)]
    pub subfolder: Option<PathBuf>,
    /// The commit installed.
    pub commit: Commit,
}

/// Where a plugin came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Origin {
    /// A folder the user picked, copied in.
    Local,
    /// A Git repository.
    Git(GitOrigin),
    /// A folder of a development plugins folder that was never installed.
    Dev,
    /// A plugin that ships with the app: always allowed, replaced by the app's
    /// copy at launch, and never removed.
    Builtin,
}

impl Origin {
    /// Whether an update from `other` replaces a plugin that came from `self`:
    /// the same repository and folder, or a local folder again.
    pub fn accepts_update_from(&self, other: &Origin) -> bool {
        match (self, other) {
            (Origin::Local, Origin::Local) | (Origin::Builtin, Origin::Builtin) => true,
            (Origin::Git(old), Origin::Git(new)) => {
                old.url == new.url && old.subfolder == new.subfolder
            }
            _ => false,
        }
    }
}

/// What the user decided about one plugin and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub enabled: bool,
    /// The permissions the user allowed, sorted.
    pub approved: Vec<Permission>,
    pub origin: Origin,
}

#[derive(Clone, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    plugins: BTreeMap<PluginId, Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_install: Option<PendingInstall>,
}

impl Default for StateFile {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            plugins: BTreeMap::new(),
            pending_install: None,
        }
    }
}

/// Whether a plugin may run, from its record and manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Run,
    Disabled,
    /// The manifest asks for these permissions the user has not allowed.
    NeedsApproval(Vec<Permission>),
}

/// What the host knows of a plugin from its record, for its status.
pub(crate) struct Assessment {
    pub verdict: Verdict,
    pub enabled: bool,
    pub approved: Vec<Permission>,
    pub origin: Origin,
}

/// The records of the plugins folder, kept in memory and in `.plugins.json`.
pub(crate) struct Managed {
    dir: PathBuf,
    policy: Policy,
    /// The ids of the plugins that ship with the app.
    builtins: BTreeSet<PluginId>,
    state: Mutex<StateFile>,
}

impl Managed {
    /// Reads the records of the plugins folder. A missing file means none; a
    /// file that cannot be read or parsed is logged and treated as empty.
    pub(crate) fn load(dir: &Path, policy: Policy) -> Self {
        let state = match std::fs::read_to_string(dir.join(STATE_FILE)) {
            Ok(text) => match serde_json::from_str::<StateFile>(&text) {
                Ok(file) => file,
                Err(error) => {
                    warn!("ignoring a corrupt {STATE_FILE}: {error}");
                    StateFile::default()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => StateFile::default(),
            Err(error) => {
                warn!("cannot read {STATE_FILE}: {error}");
                StateFile::default()
            }
        };

        Self {
            dir: dir.to_path_buf(),
            policy,
            builtins: BTreeSet::new(),
            state: Mutex::new(state),
        }
    }

    /// Reserves the ids of the plugins that ship with the app.
    pub(crate) fn with_builtins(mut self, ids: impl IntoIterator<Item = PluginId>) -> Self {
        self.builtins = ids.into_iter().collect();
        self
    }

    pub(crate) fn is_builtin_id(&self, id: &PluginId) -> bool {
        self.builtins.contains(id)
    }

    pub(crate) fn policy(&self) -> Policy {
        self.policy
    }

    pub(crate) fn record(&self, id: &PluginId) -> Option<Record> {
        locked(&self.state).plugins.get(id).cloned()
    }

    /// Whether the plugin may run, and what its status says about it.
    pub(crate) fn assess(&self, id: &PluginId, manifest: &Manifest) -> Assessment {
        let record = self
            .record(id)
            .unwrap_or_else(|| self.default_for(manifest));
        let missing: Vec<Permission> = manifest
            .permissions
            .iter()
            .filter(|permission| !record.approved.contains(permission))
            .copied()
            .collect();
        let verdict = if !missing.is_empty() {
            Verdict::NeedsApproval(missing)
        } else if record.enabled {
            Verdict::Run
        } else {
            Verdict::Disabled
        };

        Assessment {
            verdict,
            enabled: record.enabled,
            approved: record.approved,
            origin: record.origin,
        }
    }

    /// What a plugin with no record counts as.
    fn default_for(&self, manifest: &Manifest) -> Record {
        match self.policy {
            Policy::Dev => Record {
                enabled: true,
                approved: sorted(&manifest.permissions),
                origin: Origin::Dev,
            },
            Policy::Managed => Record {
                enabled: true,
                approved: Vec::new(),
                origin: Origin::Local,
            },
        }
    }

    /// Changes a plugin's record, making one first if it has none, and writes
    /// the file. The change is undone when the write fails.
    pub(crate) fn update(
        &self,
        id: &PluginId,
        manifest: Option<&Manifest>,
        change: impl FnOnce(&mut Record),
    ) -> io::Result<()> {
        let mut state = locked(&self.state);
        self.rollback_install(&mut state)?;
        let before = state.plugins.get(id).cloned();
        let mut record = before.clone().unwrap_or_else(|| match manifest {
            Some(manifest) => self.default_for(manifest),
            None => Record {
                enabled: true,
                approved: Vec::new(),
                origin: Origin::Local,
            },
        });
        change(&mut record);
        state.plugins.insert(id.clone(), record);

        let saved = self.write(&state);
        if saved.is_err() {
            match before {
                Some(record) => state.plugins.insert(id.clone(), record),
                None => state.plugins.remove(id),
            };
        }

        saved
    }

    /// Forgets a plugin's record and writes the file.
    pub(crate) fn forget(&self, id: &PluginId) -> io::Result<()> {
        let mut state = locked(&self.state);
        self.rollback_install(&mut state)?;
        let Some(before) = state.plugins.remove(id) else {
            return Ok(());
        };

        let saved = self.write(&state);
        if saved.is_err() {
            state.plugins.insert(id.clone(), before);
        }

        saved
    }

    pub(crate) fn recover_installs(&self) -> io::Result<()> {
        self.rollback_install(&mut locked(&self.state))?;
        clean_leftovers(&self.dir);
        Ok(())
    }

    pub(crate) fn commit_install(
        &self,
        id: &PluginId,
        staged: &Path,
        record: impl FnOnce(Option<&Record>, bool) -> Result<Record, ManageError>,
    ) -> Result<(), ManageError> {
        let mut state = locked(&self.state);
        let swap = InstallSwap::new(&self.dir, id);
        self.rollback_install(&mut state)
            .and_then(|()| swap.recover_backup())
            .map_err(ManageError::io("cannot recover the previous install"))?;
        let replacing = swap
            .replacing()
            .map_err(ManageError::io("cannot inspect the plugin's folder"))?;
        let record = record(state.plugins.get(id), replacing)?;

        state.pending_install = Some(PendingInstall {
            id: id.clone(),
            replacing,
        });
        if let Err(error) = self.write(&state) {
            state.pending_install = None;
            return Err(ManageError::io("cannot record the pending install")(error));
        }

        let installed = swap.place(staged, replacing).and_then(|()| {
            let mut committed = state.clone();
            committed.plugins.insert(id.clone(), record);
            committed.pending_install = None;
            self.write(&committed)
                .map_err(ManageError::io("cannot save the plugin's record"))?;
            *state = committed;
            Ok(())
        });

        if installed.is_err() {
            if let Err(error) = self.rollback_install(&mut state) {
                warn!("cannot roll back the install of {id}: {error}");
            }
        } else if let Err(error) = swap.recover_backup() {
            warn!("cannot clear the backup of {id}: {error}");
        }

        installed
    }

    fn rollback_install(&self, state: &mut StateFile) -> io::Result<()> {
        let Some(pending) = &state.pending_install else {
            return Ok(());
        };
        InstallSwap::new(&self.dir, &pending.id).rollback(pending.replacing)?;
        let pending = state.pending_install.take();

        if let Err(error) = self.write(state) {
            state.pending_install = pending;
            return Err(error);
        }

        Ok(())
    }

    /// Writes a temporary file next to the state file, then renames it over, so
    /// a crash leaves the old file or the new one.
    fn write(&self, state: &StateFile) -> io::Result<()> {
        let text = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        let temporary = self.dir.join(STATE_TMP);

        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, self.dir.join(STATE_FILE)).inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
    }
}

/// `permissions` sorted, each once.
pub(crate) fn sorted(permissions: &[Permission]) -> Vec<Permission> {
    let mut all = permissions.to_vec();
    all.sort();
    all.dedup();

    all
}

#[cfg(test)]
#[path = "tests/managed.rs"]
mod tests;
