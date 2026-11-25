use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::ident::{Permission, PluginId};
use crate::managed::{Managed, Origin, Policy, Record, sorted};
use crate::manifest::Manifest;
use crate::swap::{BUILTIN_PREFIX, MAX_PLUGIN_BYTES, ManageError, check_tree, holds, write_files};

/// A plugin that ships inside the app: its id and its files, with paths
/// relative to the plugin's folder.
pub struct Builtin {
    pub id: PluginId,
    pub files: &'static [(&'static str, &'static [u8])],
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

#[cfg(test)]
#[path = "tests/install.rs"]
mod tests;
