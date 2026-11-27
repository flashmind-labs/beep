//! A phone has no tray: only what the rest of the app expects of it.

use tauri::AppHandle;

pub(crate) fn install(_: &tauri::App) -> tauri::Result<()> {
    Ok(())
}

pub(crate) fn refresh(_: &AppHandle) {}

/// A phone has no Option taps; the switch is only drawn on a Mac.
#[tauri::command]
pub(crate) fn set_option_taps(prefs: tauri::State<'_, crate::beep::state::Prefs>, enabled: bool) {
    prefs.set_option_taps(enabled);
}
