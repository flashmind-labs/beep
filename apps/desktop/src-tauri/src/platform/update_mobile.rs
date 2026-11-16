//! A phone updates through its store; these stand in for the desktop updater.

use std::sync::Mutex;
use tauri::AppHandle;

#[derive(Default)]
pub struct Updates(#[allow(dead_code)] Mutex<()>);

pub async fn follow(_: AppHandle) {}

pub fn install(_: &AppHandle) {}

pub fn follow_feed(_: &AppHandle) {}

/// Only the native macOS updater requests an exit without an exit code.
pub fn relaunching(_app: &tauri::AppHandle) -> bool {
    false
}
