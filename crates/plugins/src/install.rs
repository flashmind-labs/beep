use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::managed::{Managed, Origin, Policy, Record, sorted};
use crate::manifest::{Manifest, ManifestError, Permission, PluginId};

/// The most a plugin folder may hold, in bytes.
pub const MAX_PLUGIN_BYTES: u64 = 16 * 1024 * 1024;

/// Where a built-in is written before it replaces the copy on disk.
const BUILTIN_PREFIX: &str = ".builtin-";

/// Where a replaced plugin waits until its replacement is in place.
const OLD_PREFIX: &str = ".old-";

/// A plugin that ships inside the app: its id and its files, with paths
/// relative to the plugin's folder.
pub struct Builtin {
    pub id: PluginId,
    pub files: &'static [(&'static str, &'static [u8])],
}

#[derive(Debug, Error)]
pub enum ManageError {
    #[error("no plugin {0} is installed")]
    Unknown(PluginId),
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("a plugin with id {0} is already installed from another source")]
    AlreadyInstalled(PluginId),
    #[error("{0} ships with Beep, so it cannot be installed over or removed")]
    Builtin(PluginId),
    #[error("the plugin is larger than {} MB", MAX_PLUGIN_BYTES / (1024 * 1024))]
    TooLarge,
    #[error("{} is a symlink that leaves the plugin folder", .0.display())]
    Symlink(PathBuf),
    #[error("{what}: {source}")]
    Io {
        what: String,
        source: std::io::Error,
    },
}

impl ManageError {
    pub(crate) fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> Self {
        let what = what.into();
        move |source| Self::Io { what, source }
    }
}

/// What installing or allowing a plugin would do, shown before the user agrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: PluginId,
    pub name: String,
    pub version: semver::Version,
    pub description: String,
    pub permissions: Vec<Permission>,
    pub source: Origin,
    pub kind: PreviewKind,
    /// The version installed now, for an update.
    pub previous_version: Option<semver::Version>,
    /// Permissions asked for that are not allowed yet.
    pub added: Vec<Permission>,
    /// Permissions allowed that the plugin no longer asks for.
    pub removed: Vec<Permission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewKind {
    /// Not installed yet.
    New,
    /// Replaces the installed version.
    Update,
    /// Already installed and waiting for its permissions.
    Allow,
}

impl Managed {
    /// Checks a prepared folder and says what installing it would do.
    pub(crate) fn preview_install(
        &self,
        dir: &Path,
        staged: &Path,
        origin: &Origin,
    ) -> Result<Preview, ManageError> {
        let manifest = self.staged_manifest(staged, origin)?;
        let target = dir.join(manifest.id.to_string());
        let record = self.record(&manifest.id);
        check_update(&manifest.id, origin, target.exists(), record.as_ref())?;

        let (kind, previous_version, approved) = match record {
            Some(record) => {
                let previous = Manifest::read(&target).ok().map(|old| old.version);
                (PreviewKind::Update, previous, record.approved)
            }
            None => (PreviewKind::New, None, Vec::new()),
        };

        Ok(preview(
            &manifest,
            origin.clone(),
            kind,
            previous_version,
            &approved,
        ))
    }

    /// What allowing an installed plugin would grant.
    pub(crate) fn preview_allow(&self, dir: &Path, id: &PluginId) -> Result<Preview, ManageError> {
        let folder = dir.join(id.to_string());
        if !folder.is_dir() {
            return Err(ManageError::Unknown(id.clone()));
        }

        let manifest = Manifest::read(&folder)?;
        let assessed = self.assess(id, &manifest);

        Ok(preview(
            &manifest,
            assessed.origin,
            PreviewKind::Allow,
            None,
            &assessed.approved,
        ))
    }

    /// Moves a prepared folder in as plugin `id`, replacing an update's old
    /// folder, and records its origin and the permissions the user allowed.
    pub(crate) fn install(&self, staged: &Path, origin: Origin) -> Result<PluginId, ManageError> {
        let manifest = self.staged_manifest(staged, &origin)?;
        let id = manifest.id;
        self.commit_install(&id, staged, |before, replacing| {
            check_update(&id, &origin, replacing, before)?;
            Ok(Record {
                enabled: before.map_or(origin != Origin::Builtin, |record| record.enabled),
                approved: sorted(&manifest.permissions),
                origin,
            })
        })?;

        Ok(id)
    }

    fn staged_manifest(&self, staged: &Path, origin: &Origin) -> Result<Manifest, ManageError> {
        let manifest = Manifest::load(staged)?;
        check_tree(staged, MAX_PLUGIN_BYTES)?;

        if self.is_builtin_id(&manifest.id) && *origin != Origin::Builtin {
            return Err(ManageError::Builtin(manifest.id));
        }

        Ok(manifest)
    }

    /// Makes the plugins folder hold the app's copy of each built-in, with its
    /// record: allowed for what its manifest asks, off until the user turns it
    /// on. A copy that differs from the app's is replaced. A development folder
    /// is left alone.
    pub(crate) fn sync_builtins(&self, dir: &Path, builtins: &[Builtin]) {
        if self.policy() == Policy::Dev {
            return;
        }

        for builtin in builtins {
            if let Err(error) = self.sync_builtin(dir, builtin) {
                log::warn!("cannot install the built-in plugin {}: {error}", builtin.id);
            }
        }
    }

    fn sync_builtin(&self, dir: &Path, builtin: &Builtin) -> Result<(), ManageError> {
        let recorded = self
            .record(&builtin.id)
            .is_some_and(|record| record.origin == Origin::Builtin);
        if recorded && holds(&dir.join(builtin.id.to_string()), builtin.files) {
            return Ok(());
        }

        let staged = dir.join(format!("{BUILTIN_PREFIX}{}", builtin.id));
        let _ = std::fs::remove_dir_all(&staged);
        let installed = write_files(&staged, builtin.files)
            .and_then(|()| self.install(&staged, Origin::Builtin));
        if installed.is_err() {
            let _ = std::fs::remove_dir_all(&staged);
        }

        installed.map(drop)
    }
}

fn check_update(
    id: &PluginId,
    origin: &Origin,
    installed: bool,
    record: Option<&Record>,
) -> Result<(), ManageError> {
    // The app's own copy replaces whatever holds its id.
    if *origin != Origin::Builtin
        && record.map_or(installed, |record| {
            !record.origin.accepts_update_from(origin)
        })
    {
        return Err(ManageError::AlreadyInstalled(id.clone()));
    }

    Ok(())
}

fn preview(
    manifest: &Manifest,
    source: Origin,
    kind: PreviewKind,
    previous_version: Option<semver::Version>,
    approved: &[Permission],
) -> Preview {
    let asked: BTreeSet<Permission> = manifest.permissions.iter().copied().collect();
    let allowed: BTreeSet<Permission> = approved.iter().copied().collect();

    Preview {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        permissions: sorted(&manifest.permissions),
        source,
        kind,
        previous_version,
        added: asked.difference(&allowed).copied().collect(),
        removed: allowed.difference(&asked).copied().collect(),
    }
}

/// Recorded before either rename. The plugin's old record stays in place
/// until the install commits and removes this marker in the same write.
#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct PendingInstall {
    pub(crate) id: PluginId,
    pub(crate) replacing: bool,
}

pub(crate) struct InstallSwap {
    target: PathBuf,
    backup: PathBuf,
}

impl InstallSwap {
    pub(crate) fn new(dir: &Path, id: &PluginId) -> Self {
        Self {
            target: dir.join(id.to_string()),
            backup: dir.join(format!("{OLD_PREFIX}{id}")),
        }
    }

    pub(crate) fn replacing(&self) -> io::Result<bool> {
        self.target.try_exists()
    }

    pub(crate) fn place(&self, staged: &Path, replacing: bool) -> Result<(), ManageError> {
        if replacing {
            std::fs::rename(&self.target, &self.backup)
                .map_err(ManageError::io("cannot replace the old version"))?;
        }

        std::fs::rename(staged, &self.target)
            .map_err(ManageError::io("cannot move the plugin into place"))
    }

    pub(crate) fn rollback(&self, replacing: bool) -> io::Result<()> {
        if self.backup.try_exists()? {
            remove_folder(&self.target)?;
            std::fs::rename(&self.backup, &self.target)?;
        } else if !replacing {
            remove_folder(&self.target)?;
        }

        Ok(())
    }

    /// Recovers backups left by a committed install or a version without a journal.
    pub(crate) fn recover_backup(&self) -> io::Result<()> {
        if self.target.try_exists()? {
            remove_folder(&self.backup)
        } else if self.backup.try_exists()? {
            std::fs::rename(&self.backup, &self.target)
        } else {
            Ok(())
        }
    }
}

fn remove_folder(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn write_files(root: &Path, files: &[(&str, &[u8])]) -> Result<(), ManageError> {
    for (name, bytes) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ManageError::io(format!(
                "cannot create {}",
                parent.display()
            )))?;
        }
        std::fs::write(&path, bytes)
            .map_err(ManageError::io(format!("cannot write {}", path.display())))?;
    }

    Ok(())
}

/// Whether the folder holds exactly these files.
fn holds(root: &Path, files: &[(&str, &[u8])]) -> bool {
    let same = files
        .iter()
        .all(|(name, bytes)| std::fs::read(root.join(name)).is_ok_and(|found| found == *bytes));

    same && count_files(root) == files.len()
}

fn count_files(root: &Path) -> usize {
    let mut count = 0;
    let mut pending = vec![root.to_path_buf()];

    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                count += 1;
            }
        }
    }

    count
}

/// Cleans up staged built-ins and recovers backups without a pending install.
pub(crate) fn clean_leftovers(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(id) = name.strip_prefix(OLD_PREFIX).and_then(|id| id.parse().ok()) {
            if let Err(error) = InstallSwap::new(dir, &id).recover_backup() {
                log::warn!("cannot recover plugin {id}: {error}");
            }
        } else if name.starts_with(BUILTIN_PREFIX) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Fails when the folder holds more than `limit` bytes, or has a symlink that
/// points outside it or nowhere.
pub fn check_tree(root: &Path, limit: u64) -> Result<(), ManageError> {
    let canonical = root
        .canonicalize()
        .map_err(ManageError::io(format!("cannot read {}", root.display())))?;
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];

    while let Some(folder) = pending.pop() {
        let entries = std::fs::read_dir(&folder)
            .map_err(ManageError::io(format!("cannot read {}", folder.display())))?;
        for entry in entries {
            let entry = entry.map_err(ManageError::io("cannot list a plugin folder"))?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)
                .map_err(ManageError::io(format!("cannot read {}", path.display())))?;

            if meta.file_type().is_symlink() {
                let inside = path
                    .canonicalize()
                    .is_ok_and(|target| target.starts_with(&canonical));
                if !inside {
                    let shown = path.strip_prefix(root).unwrap_or(&path).to_path_buf();

                    return Err(ManageError::Symlink(shown));
                }
            } else if meta.is_dir() {
                pending.push(path);
            } else {
                total = total.saturating_add(meta.len());
                if total > limit {
                    return Err(ManageError::TooLarge);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "tests/install.rs"]
mod tests;
