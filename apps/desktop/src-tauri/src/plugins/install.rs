//! Getting a plugin into the plugins folder with the user's consent: stage it
//! in a dot folder, show a preview, then move it in or throw it away. Nothing
//! staged runs or counts as installed.

use crate::store::lock;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;

use beep_plugins::{
    Commit, GitOrigin, Host, MAX_PLUGIN_BYTES, ManageError, ManifestError, Origin, PluginId,
    Preview, check_tree,
};
use serde::{Serialize, Serializer};
use strum::IntoStaticStr;
use thiserror::Error;
use uuid::Uuid;

use super::git::{Git, GitError, GitRef, InvalidInput, Remote, RepoUrl, Subfolder};

/// Folders of this name hold a plugin that is not installed yet.
const STAGE_PREFIX: &str = ".stage-";

/// What a plugins command tells the page when it fails. The page reads `kind`
/// and shows `message`.
#[derive(Debug, Error, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum PluginsError {
    #[error("Plugins are not running, so they cannot be changed.")]
    NotRunning,
    #[error("{0}")]
    Invalid(#[from] InvalidInput),
    #[error("{0}")]
    Git(#[from] GitError),
    #[error("There is no {0} in the repository. Check the folder, or leave it empty for the root.")]
    NoManifest(String),
    #[error("The plugin's beep-plugin.json is not valid: {0}")]
    InvalidManifest(String),
    #[error("The plugin is larger than {} MB, so it was not installed.", MAX_PLUGIN_BYTES / (1024 * 1024))]
    TooLarge,
    #[error(
        "The plugin has a link to a file outside its own folder ({0}), so it was not installed."
    )]
    UnsafeLink(String),
    #[error("A plugin with the id {0} is already installed from another source. Remove it first.")]
    AlreadyInstalled(PluginId),
    #[error("{0} ships with Beep, so it cannot be installed over or removed.")]
    Builtin(PluginId),
    #[error("No plugin with the id {0} is installed.")]
    UnknownPlugin(PluginId),
    #[error("This plugin was not installed from Git, so it has no repository to update from.")]
    NotFromGit,
    #[error("That install was already finished or cancelled. Start it again.")]
    UnknownStage,
    #[error("{0}")]
    Disk(String),
}

impl Serialize for PluginsError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut wire = serializer.serialize_struct("PluginsError", 2)?;
        wire.serialize_field("kind", Into::<&'static str>::into(self))?;
        wire.serialize_field("message", &self.to_string())?;
        wire.end()
    }
}

impl From<ManageError> for PluginsError {
    fn from(error: ManageError) -> Self {
        match error {
            ManageError::Unknown(id) => Self::UnknownPlugin(id),
            ManageError::Manifest(ManifestError::Read { path, .. }) => {
                let name = path.file_name().map_or_else(
                    || "beep-plugin.json".to_owned(),
                    |n| n.to_string_lossy().into(),
                );
                Self::NoManifest(name)
            }
            ManageError::Manifest(other) => Self::InvalidManifest(other.to_string()),
            ManageError::AlreadyInstalled(id) => Self::AlreadyInstalled(id),
            ManageError::Builtin(id) => Self::Builtin(id),
            ManageError::TooLarge => Self::TooLarge,
            ManageError::Symlink(path) => Self::UnsafeLink(path.display().to_string()),
            ManageError::Io { what, source } => Self::Disk(format!("{what}: {source}")),
        }
    }
}

impl PluginsError {
    fn disk(what: &str) -> impl FnOnce(std::io::Error) -> Self {
        let what = what.to_owned();
        move |source| Self::Disk(format!("{what}: {source}"))
    }
}

/// One staged install, named by the page when it confirms or cancels.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StageId(Uuid);

impl StageId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }

    fn folder(&self) -> String {
        format!("{STAGE_PREFIX}{}", self.0.simple())
    }
}

impl FromStr for StageId {
    type Err = PluginsError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(text)
            .map(Self)
            .map_err(|_| PluginsError::UnknownStage)
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for StageId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// What a person typed to install from Git, checked.
#[derive(Debug, Clone)]
pub(crate) struct GitRequest {
    pub url: RepoUrl,
    pub reference: Option<GitRef>,
    pub subfolder: Option<Subfolder>,
}

impl GitRequest {
    /// Empty text for the reference or folder means none.
    pub(crate) fn parse(
        repository: &str,
        reference: &str,
        folder: &str,
    ) -> Result<Self, PluginsError> {
        let reference = (!reference.trim().is_empty())
            .then(|| GitRef::parse(reference))
            .transpose()?;
        let subfolder = (!folder.trim().is_empty())
            .then(|| Subfolder::parse(folder))
            .transpose()?;

        Ok(Self {
            url: RepoUrl::parse(repository)?,
            reference,
            subfolder,
        })
    }

    /// The request that made an installed plugin, to ask its source again.
    pub(crate) fn of(origin: &GitOrigin) -> Result<Self, PluginsError> {
        Ok(Self {
            url: RepoUrl::stored(&origin.url)?,
            reference: origin.reference.as_deref().map(GitRef::parse).transpose()?,
            subfolder: origin
                .subfolder
                .as_deref()
                .map(|path| Subfolder::parse(&path.to_string_lossy()))
                .transpose()?,
        })
    }
}

/// A plugin fetched or copied into a staging folder, not yet previewed.
#[derive(Debug)]
pub(crate) struct Fetched {
    stage: StageId,
    /// `<plugins>/.stage-x/<id>`.
    plugin: PathBuf,
    origin: Origin,
}

/// A staged install and what it would do, for the page's consent sheet.
#[derive(Debug, Serialize)]
pub(crate) struct Staged {
    pub stage: StageId,
    pub preview: Preview,
}

/// Whether a Git plugin's source has a newer commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub(crate) enum UpdateCheck {
    Current,
    Available {
        commit: Commit,
    },
    /// Installed at a commit, so there is nothing newer to find.
    Pinned,
}

struct Stage {
    plugin: PathBuf,
    origin: Origin,
}

/// Removes the staging folder a staged plugin sits in.
fn discard(plugin: &Path) {
    if let Some(folder) = plugin.parent() {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// The installs waiting for the user's answer.
pub(crate) struct Installer {
    git: Git,
    stages: Mutex<HashMap<StageId, Stage>>,
}

impl Installer {
    pub(crate) fn new() -> Self {
        Self::with(Git::new())
    }

    fn with(git: Git) -> Self {
        Self {
            git,
            stages: Mutex::new(HashMap::new()),
        }
    }

    /// Fetches a plugin's repository into a staging folder in `plugins` and finds
    /// the plugin in it. Nothing is installed.
    pub(crate) async fn fetch(
        &self,
        plugins: &Path,
        request: &GitRequest,
    ) -> Result<Fetched, PluginsError> {
        let stage = StageId::new();
        let folder = plugins.join(stage.folder());
        let work = async {
            let repo = folder.join("repo");
            std::fs::create_dir_all(&folder)
                .map_err(PluginsError::disk("cannot prepare a folder"))?;
            let commit = self
                .git
                .fetch(&request.url, request.reference.as_ref(), &repo)
                .await?;
            let origin = Origin::Git(GitOrigin {
                url: request.url.to_string(),
                reference: request
                    .reference
                    .as_ref()
                    .map(|name| name.as_str().to_owned()),
                subfolder: request
                    .subfolder
                    .as_ref()
                    .map(|path| path.path().to_path_buf()),
                commit,
            });
            let subfolder = request
                .subfolder
                .as_ref()
                .map(|path| path.path().to_path_buf());
            let (folder_for_work, repo_for_work) = (folder.clone(), repo.clone());
            let plugin = tokio::task::spawn_blocking(move || {
                place(&repo_for_work, subfolder.as_deref(), &folder_for_work)
            })
            .await
            .map_err(|e| PluginsError::Disk(e.to_string()))??;

            Ok(Fetched {
                stage: stage.clone(),
                plugin,
                origin,
            })
        };

        let done: Result<Fetched, PluginsError> = work.await;
        if done.is_err() {
            let _ = std::fs::remove_dir_all(&folder);
        }

        done
    }

    /// Copies a folder the user picked into a staging folder in `plugins`.
    pub(crate) fn copy_local(
        &self,
        plugins: &Path,
        source: &Path,
    ) -> Result<Fetched, PluginsError> {
        let manifest = beep_plugins::Manifest::load(source).map_err(ManageError::from)?;
        let stage = StageId::new();
        let folder = plugins.join(stage.folder());
        let plugin = folder.join(manifest.id.to_string());

        let copied = std::fs::create_dir_all(&folder)
            .map_err(PluginsError::disk("cannot prepare a folder"))
            .and_then(|()| copy_tree(source, &plugin, MAX_PLUGIN_BYTES))
            .and_then(|()| check_tree(&plugin, MAX_PLUGIN_BYTES).map_err(PluginsError::from));
        if let Err(error) = copied {
            let _ = std::fs::remove_dir_all(&folder);

            return Err(error);
        }

        Ok(Fetched {
            stage,
            plugin,
            origin: Origin::Local,
        })
    }

    /// Says what installing a staged plugin would do and keeps it for the answer.
    /// A staged plugin that cannot be installed is thrown away.
    pub(crate) fn preview(&self, host: &Host, fetched: Fetched) -> Result<Staged, PluginsError> {
        let Fetched {
            stage,
            plugin,
            origin,
        } = fetched;
        match host.preview_install(&plugin, &origin) {
            Ok(preview) => {
                self.stages()
                    .insert(stage.clone(), Stage { plugin, origin });

                Ok(Staged { stage, preview })
            }
            Err(error) => {
                discard(&plugin);

                Err(error.into())
            }
        }
    }

    /// Installs a staged plugin the user agreed to. The staging folder goes either way.
    pub(crate) fn confirm(&self, host: &Host, stage: &StageId) -> Result<PluginId, PluginsError> {
        let staged = self
            .stages()
            .remove(stage)
            .ok_or(PluginsError::UnknownStage)?;
        let installed = host.install(&staged.plugin, staged.origin);
        discard(&staged.plugin);

        Ok(installed?)
    }

    /// Throws a staged plugin away.
    pub(crate) fn cancel(&self, stage: &StageId) {
        if let Some(staged) = self.stages().remove(stage) {
            discard(&staged.plugin);
        }
    }

    /// Asks a Git plugin's repository for a newer commit than the installed one.
    pub(crate) async fn check_update(
        &self,
        origin: &GitOrigin,
    ) -> Result<UpdateCheck, PluginsError> {
        let request = GitRequest::of(origin)?;
        let newest = self
            .git
            .newest(&request.url, request.reference.as_ref())
            .await?;

        Ok(match newest {
            Remote::At(commit) if commit == origin.commit => UpdateCheck::Current,
            Remote::At(commit) => UpdateCheck::Available { commit },
            Remote::Pinned => UpdateCheck::Pinned,
        })
    }

    fn stages(&self) -> std::sync::MutexGuard<'_, HashMap<StageId, Stage>> {
        lock(&self.stages)
    }
}

/// Removes the staging folders a closed app left behind.
pub(crate) fn clean_stale(plugins: &Path) {
    let Ok(entries) = std::fs::read_dir(plugins) else {
        return;
    };

    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGE_PREFIX)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Moves the plugin of a fetched repository to `<stage>/<id>`, without the
/// repository's own files when it sits in a subfolder, and checks it.
fn place(repo: &Path, subfolder: Option<&Path>, stage: &Path) -> Result<PathBuf, PluginsError> {
    let shown = subfolder.map_or_else(
        || "beep-plugin.json".to_owned(),
        |path| format!("{}/beep-plugin.json", path.display()),
    );
    let source = match subfolder {
        Some(path) => repo.join(path),
        None => repo.to_path_buf(),
    };
    let inside = source
        .canonicalize()
        .ok()
        .zip(repo.canonicalize().ok())
        .filter(|(found, root)| found.starts_with(root));
    let Some((source, _)) = inside else {
        return Err(PluginsError::NoManifest(shown));
    };

    let manifest = beep_plugins::Manifest::load(&source).map_err(|error| match error {
        ManifestError::Read { .. } => PluginsError::NoManifest(shown),
        other => PluginsError::InvalidManifest(other.to_string()),
    })?;
    let target = stage.join(manifest.id.to_string());

    if subfolder.is_none() {
        std::fs::remove_dir_all(repo.join(".git"))
            .map_err(PluginsError::disk("cannot clean the download"))?;
    }
    std::fs::rename(&source, &target).map_err(PluginsError::disk("cannot stage the plugin"))?;
    if subfolder.is_some() {
        std::fs::remove_dir_all(repo).map_err(PluginsError::disk("cannot clean the download"))?;
    }

    check_tree(&target, MAX_PLUGIN_BYTES)?;

    Ok(target)
}

/// Copies a folder without its `.git`, refusing links that leave it and
/// stopping once it holds more than `limit` bytes.
fn copy_tree(source: &Path, target: &Path, limit: u64) -> Result<(), PluginsError> {
    let root = source
        .canonicalize()
        .map_err(PluginsError::disk("cannot read the folder"))?;
    let mut total = 0u64;
    let mut pending = vec![(source.to_path_buf(), target.to_path_buf())];

    while let Some((from, to)) = pending.pop() {
        std::fs::create_dir_all(&to).map_err(PluginsError::disk("cannot copy the folder"))?;
        let entries =
            std::fs::read_dir(&from).map_err(PluginsError::disk("cannot read the folder"))?;

        for entry in entries {
            let entry = entry.map_err(PluginsError::disk("cannot read the folder"))?;
            let (path, name) = (entry.path(), entry.file_name());
            if from == source && name == ".git" {
                continue;
            }

            let meta = std::fs::symlink_metadata(&path)
                .map_err(PluginsError::disk("cannot read the folder"))?;
            let kind = meta.file_type();
            let shown = || {
                path.strip_prefix(source)
                    .unwrap_or(&path)
                    .display()
                    .to_string()
            };
            let destination = to.join(&name);

            if kind.is_symlink() {
                let target_inside = path
                    .canonicalize()
                    .ok()
                    .filter(|found| found.starts_with(&root) && found.is_file());
                if target_inside.is_none() {
                    return Err(PluginsError::UnsafeLink(shown()));
                }
            }

            if kind.is_dir() {
                pending.push((path, destination));
            } else if kind.is_file() || kind.is_symlink() {
                let copied = std::fs::copy(&path, &destination)
                    .map_err(PluginsError::disk("cannot copy a file"))?;
                total = total.saturating_add(copied);
                if total > limit {
                    return Err(PluginsError::TooLarge);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "../tests/plugins_install.rs"]
mod tests;
