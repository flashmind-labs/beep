//! A phone has no tray: only what the rest of the app expects of it.

use tauri::AppHandle;

pub(crate) fn install(_: &tauri::App) -> tauri::Result<()> {
    Ok(())
}

pub(crate) fn refresh(_: &AppHandle) {}
