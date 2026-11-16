//! The commands of the Settings page that change plugins: turn one off, allow
//! it, remove it, and install or update one through a preview the user confirms.

use std::path::PathBuf;

use beep_plugins::{Host, Origin, PluginId, Preview};
use tauri::{AppHandle, Manager, State};

use super::install::{GitRequest, Installer, PluginsError, StageId, Staged, UpdateCheck};

/// Runs `work` on a worker that may block, with the plugin host and installer.
async fn blocking<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(&Host, &Installer) -> Result<T, PluginsError> + Send + 'static,
) -> Result<T, PluginsError> {
    let app = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let host = app.try_state::<Host>().ok_or(PluginsError::NotRunning)?;
        let installer = app.state::<Installer>();

        work(&host, &installer)
    })
    .await
    .map_err(|e| PluginsError::Disk(e.to_string()))?
}

fn plugins_folder(app: &AppHandle) -> Result<PathBuf, PluginsError> {
    let host = app.try_state::<Host>().ok_or(PluginsError::NotRunning)?;

    Ok(host.dir().to_path_buf())
}

/// Turns a plugin on or off.
#[tauri::command]
pub(crate) async fn plugins_enable(
    app: AppHandle,
    id: PluginId,
    enabled: bool,
) -> Result<(), PluginsError> {
    blocking(&app, move |host, _| Ok(host.set_enabled(&id, enabled)?)).await
}

/// What allowing an installed plugin would grant, for the consent sheet.
#[tauri::command]
pub(crate) async fn plugins_review(app: AppHandle, id: PluginId) -> Result<Preview, PluginsError> {
    blocking(&app, move |host, _| Ok(host.preview_allow(&id)?)).await
}

/// Allows what the plugin's manifest asks for and turns it on. The page calls
/// this only after the user read the consent sheet.
#[tauri::command]
pub(crate) async fn plugins_allow(app: AppHandle, id: PluginId) -> Result<(), PluginsError> {
    blocking(&app, move |host, _| Ok(host.approve(&id)?)).await
}

/// Stops a plugin and deletes its folder and record.
#[tauri::command]
pub(crate) async fn plugins_remove(app: AppHandle, id: PluginId) -> Result<(), PluginsError> {
    blocking(&app, move |host, _| Ok(host.remove(&id)?)).await
}

/// Fetches a plugin from a Git repository into a staging folder and says what
/// installing it would do. Nothing is installed until `plugins_confirm`.
#[tauri::command]
pub(crate) async fn plugins_stage_git(
    app: AppHandle,
    installer: State<'_, Installer>,
    repository: String,
    reference: String,
    folder: String,
) -> Result<Staged, PluginsError> {
    let request = GitRequest::parse(&repository, &reference, &folder)?;
    let plugins = plugins_folder(&app)?;
    let fetched = installer.fetch(&plugins, &request).await?;

    blocking(&app, move |host, installer| {
        installer.preview(host, fetched)
    })
    .await
}

/// Copies `folder`, one picked on this computer, into a staging folder and says what installing
/// it would do.
#[tauri::command]
pub(crate) async fn plugins_stage_folder(
    app: AppHandle,
    folder: PathBuf,
) -> Result<Staged, PluginsError> {
    let plugins = plugins_folder(&app)?;

    if !folder.is_absolute() || !folder.is_dir() {
        return Err(PluginsError::Disk(format!(
            "{} is not a folder on this computer.",
            folder.display()
        )));
    }

    blocking(&app, move |host, installer| {
        let fetched = installer.copy_local(&plugins, &folder)?;

        installer.preview(host, fetched)
    })
    .await
}

/// Asks the repository a Git plugin came from whether it has a newer commit.
#[tauri::command]
pub(crate) async fn plugins_check_update(
    app: AppHandle,
    installer: State<'_, Installer>,
    id: PluginId,
) -> Result<UpdateCheck, PluginsError> {
    let origin = git_origin(&app, &id)?;

    installer.check_update(&origin).await
}

/// Fetches the newest commit of a Git plugin's source into a staging folder and
/// says what changes, including permissions asked for or dropped.
#[tauri::command]
pub(crate) async fn plugins_stage_update(
    app: AppHandle,
    installer: State<'_, Installer>,
    id: PluginId,
) -> Result<Staged, PluginsError> {
    let origin = git_origin(&app, &id)?;
    let request = GitRequest::of(&origin)?;
    let plugins = plugins_folder(&app)?;
    let fetched = installer.fetch(&plugins, &request).await?;

    blocking(&app, move |host, installer| {
        installer.preview(host, fetched)
    })
    .await
}

/// Installs a staged plugin, with the permissions its preview showed.
#[tauri::command]
pub(crate) async fn plugins_confirm(
    app: AppHandle,
    stage: String,
) -> Result<PluginId, PluginsError> {
    let stage: StageId = stage.parse()?;

    blocking(&app, move |host, installer| installer.confirm(host, &stage)).await
}

/// Throws a staged plugin away.
#[tauri::command]
pub(crate) fn plugins_cancel(
    installer: State<'_, Installer>,
    stage: String,
) -> Result<(), PluginsError> {
    installer.cancel(&stage.parse()?);

    Ok(())
}

/// Shows the plugins folder in the file manager.
#[tauri::command]
pub(crate) fn plugins_open_folder(app: AppHandle) -> Result<(), PluginsError> {
    let folder = plugins_folder(&app)?;

    crate::platform::open(folder).map_err(|e| PluginsError::Disk(e.to_string()))
}

/// Where a Git plugin came from, or why it has no source to ask.
fn git_origin(app: &AppHandle, id: &PluginId) -> Result<beep_plugins::GitOrigin, PluginsError> {
    let host = app.try_state::<Host>().ok_or(PluginsError::NotRunning)?;
    let record = host
        .record(id)
        .ok_or_else(|| PluginsError::UnknownPlugin(id.clone()))?;

    match record.origin {
        Origin::Git(origin) => Ok(origin),
        Origin::Local | Origin::Dev | Origin::Builtin => Err(PluginsError::NotFromGit),
    }
}
