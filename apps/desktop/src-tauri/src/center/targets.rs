//! Handing the main window a view to show: what the bar, the tray, a
//! notice or another window asks it to open. Entry point: `open_on`.

use super::window::{LABEL, open};
use crate::error::Result;
use crate::targets::CenterTarget;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Runtime, State};

/// Report a Problem, in the tray menu and the app's Help menu.
#[cfg(desktop)]
pub(crate) const REPORT: &str = "report-problem";

/// The view the window goes to next. It is read once, so a reload does not
/// go back to it.
#[derive(Debug, Default)]
pub(crate) struct Pending(Mutex<Option<CenterTarget>>);

impl Pending {
    fn put(&self, target: CenterTarget) {
        *crate::store::lock(&self.0) = Some(target);
    }

    fn take(&self) -> Option<CenterTarget> {
        crate::store::lock(&self.0).take()
    }
}

/// Opens the window on `target`. A window that is still loading reads it
/// with `center_pending`; one already open hears `center-open`.
pub(crate) fn open_on<R: Runtime>(
    app: &AppHandle<R>,
    pending: &Pending,
    target: CenterTarget,
) -> Result<()> {
    pending.put(target.clone());
    open(app)?;
    // A window still loading reads `Pending` instead, so a lost event only
    // goes to the log.
    if let Err(e) = app.emit_to(LABEL, "center-open", &target) {
        log::warn!("telling the command center what to open: {e}");
    }

    Ok(())
}

#[tauri::command]
pub fn center_open(
    app: AppHandle,
    pending: State<'_, Pending>,
    target: CenterTarget,
) -> Result<()> {
    open_on(&app, &pending, target)
}

/// The view the window was asked to show, once.
#[tauri::command]
pub fn center_pending(pending: State<'_, Pending>) -> Option<CenterTarget> {
    pending.take()
}

#[cfg(test)]
#[path = "../tests/center_targets.rs"]
mod tests;
