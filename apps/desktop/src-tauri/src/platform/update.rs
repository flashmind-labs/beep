//! Updates from the release bucket: checked shortly
//! after launch and then every few hours, downloaded quietly, and offered
//! as "Restart to Update Beep" in the tray menu. Quitting installs a
//! downloaded update too.

use crate::{notices::notify, tray};
use std::{
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// A downloaded update waiting for a restart or quit.
struct Ready {
    update: Update,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Updates(Mutex<Option<Ready>>);

impl Updates {
    fn lock(&self) -> MutexGuard<'_, Option<Ready>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The version of the downloaded update, if one is waiting.
pub fn ready(app: &AppHandle) -> Option<String> {
    app.state::<Updates>()
        .lock()
        .as_ref()
        .map(|ready| ready.update.version.clone())
}

pub async fn follow(app: AppHandle) {
    // A build from source has no signed release to update from.
    if cfg!(debug_assertions) {
        return;
    }
    tokio::time::sleep(FIRST_CHECK).await;
    loop {
        check(&app, false).await;
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

/// Looks for a newer release and downloads it. `asked` is a check from the
/// menu, which also says when there is nothing new or the check failed.
pub async fn check(app: &AppHandle, asked: bool) {
    if ready(app).is_some() {
        return;
    }
    match download(app).await {
        Ok(Some(version)) => {
            tray::refresh(app);
            notify::text(
                app,
                &format!("Beep {version} is ready"),
                "Choose Restart to Update Beep in the menu bar, or it installs the next time Beep quits.",
            );
        }
        Ok(None) if asked => notify::text(app, "Beep is up to date", ""),
        Ok(None) => {}
        Err(e) => {
            log::warn!("checking for an update: {e}");
            if asked {
                notify::text(app, "Could not check for updates", "Try again later.");
            }
        }
    }
}

async fn download(app: &AppHandle) -> tauri_plugin_updater::Result<Option<String>> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(None);
    };
    let bytes = update.download(|_, _| {}, || {}).await?;
    let version = update.version.clone();
    log::info!("downloaded Beep {version}");
    *app.state::<Updates>().lock() = Some(Ready { update, bytes });
    Ok(Some(version))
}

/// Installs the downloaded update, if any; the next launch runs it.
pub fn install(app: &AppHandle) {
    let Some(ready) = app.state::<Updates>().lock().take() else {
        return;
    };
    if let Err(e) = ready.update.install(&ready.bytes) {
        log::warn!("installing Beep {}: {e}", ready.update.version);
    }
}

/// Installs the downloaded update and starts it.
pub fn restart(app: &AppHandle) {
    install(app);
    app.restart();
}
