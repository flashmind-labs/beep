use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ident::PluginId;
use crate::manifest::ManifestError;

/// The most a plugin folder may hold, in bytes.
pub const MAX_PLUGIN_BYTES: u64 = 16 * 1024 * 1024;

/// Where a built-in is written before it replaces the copy on disk.
pub(crate) const BUILTIN_PREFIX: &str = ".builtin-";

/// Where a replaced plugin waits until its replacement is in place.
const OLD_PREFIX: &str = ".old-";

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

pub(crate) fn write_files(root: &Path, files: &[(&str, &[u8])]) -> Result<(), ManageError> {
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
pub(crate) fn holds(root: &Path, files: &[(&str, &[u8])]) -> bool {
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
