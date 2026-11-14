use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
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
        let manifest = Manifest::load(staged)?;
        check_tree(staged, MAX_PLUGIN_BYTES)?;

        let builtin = *origin == Origin::Builtin;
        if self.is_builtin_id(&manifest.id) && !builtin {
            return Err(ManageError::Builtin(manifest.id));
        }

        let installed = dir.join(manifest.id.to_string()).exists();
        let record = self.record(&manifest.id);
        let (kind, previous_version, approved) = match record {
            // The app's own copy replaces whatever holds its id.
            Some(record) if builtin || record.origin.accepts_update_from(origin) => {
                let previous = Manifest::read(&dir.join(manifest.id.to_string()))
                    .ok()
                    .map(|old| old.version);
                (PreviewKind::Update, previous, record.approved)
            }
            Some(_) => return Err(ManageError::AlreadyInstalled(manifest.id)),
            None if installed && !builtin => {
                return Err(ManageError::AlreadyInstalled(manifest.id));
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
    pub(crate) fn install(
        &self,
        dir: &Path,
        staged: &Path,
        origin: Origin,
    ) -> Result<PluginId, ManageError> {
        let shown = self.preview_install(dir, staged, &origin)?;
        let id = shown.id.clone();
        let target = dir.join(id.to_string());
        let held = dir.join(format!("{OLD_PREFIX}{id}"));
        let before = self.record(&id);
        let enabled = before
            .as_ref()
            .map_or(origin != Origin::Builtin, |record| record.enabled);
        let approved = sorted(&shown.permissions);

        self.update(&id, None, |record| {
            record.enabled = enabled;
            record.approved = approved;
            record.origin = origin;
        })
        .map_err(ManageError::io("cannot save the plugin's record"))?;

        let moved = swap_in(staged, &target, &held);
        if let Err(error) = moved {
            self.restore(&id, before);

            return Err(error);
        }
        let _ = std::fs::remove_dir_all(&held);

        Ok(id)
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
            .and_then(|()| self.install(dir, &staged, Origin::Builtin));
        if installed.is_err() {
            let _ = std::fs::remove_dir_all(&staged);
        }

        installed.map(drop)
    }

    /// Puts a record back as it was before an install that did not finish.
    fn restore(&self, id: &PluginId, before: Option<Record>) {
        let restored = match before {
            Some(old) => self.update(id, None, |record| *record = old),
            None => self.forget(id),
        };
        if let Err(error) = restored {
            log::warn!("cannot restore the record of {id}: {error}");
        }
    }
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

/// Renames `staged` to `target`. An existing `target` moves to `held` first
/// and comes back if the rename fails.
fn swap_in(staged: &Path, target: &Path, held: &Path) -> Result<(), ManageError> {
    let _ = std::fs::remove_dir_all(held);
    let replacing = target.exists();
    if replacing {
        std::fs::rename(target, held).map_err(ManageError::io("cannot replace the old version"))?;
    }

    if let Err(error) = std::fs::rename(staged, target) {
        if replacing {
            let _ = std::fs::rename(held, target);
        }

        return Err(ManageError::io("cannot move the plugin into place")(error));
    }

    Ok(())
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

/// Removes the folders a crashed install left in the plugins folder.
pub(crate) fn clean_leftovers(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(OLD_PREFIX) || name.starts_with(BUILTIN_PREFIX) {
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
