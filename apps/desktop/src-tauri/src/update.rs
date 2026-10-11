//! Updates from the release bucket: checked shortly after launch and then
//! every few hours, installed in the background, and applied by a restart
//! once the bar is closed.

use crate::bar;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// How often a waiting restart looks whether the bar has closed.
const RESTART_POLL: Duration = Duration::from_secs(30);

pub async fn follow(app: AppHandle) {
    // A build from source has no signed release to update from.
    if cfg!(debug_assertions) {
        return;
    }
    tokio::time::sleep(FIRST_CHECK).await;
    loop {
        match install(&app).await {
            Ok(true) => {
                while bar::is_open(&app) {
                    tokio::time::sleep(RESTART_POLL).await;
                }
                app.restart();
            }
            Ok(false) => {}
            Err(e) => log::warn!("checking for an update: {e}"),
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

/// Downloads and installs a newer release; whether there was one.
async fn install(app: &AppHandle) -> tauri_plugin_updater::Result<bool> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(false);
    };
    log::info!("installing Beep {}", update.version);
    update.download_and_install(|_, _| {}, || {}).await?;
    Ok(true)
}
