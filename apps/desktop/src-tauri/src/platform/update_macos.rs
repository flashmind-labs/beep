//! Sparkle presents its standard native UI, verifies downloads and owns installation.
//! Only release builds have it (the `sparkle` feature); others cannot update.
//! The feed in Info.plist has minor releases only; Developer mode reads the
//! feed of every release beside it.

#[cfg(feature = "sparkle")]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};
#[cfg(feature = "sparkle")]
use tauri_plugin_sparkle_updater::{SparkleUpdaterExt, events::EVENT_WILL_RELAUNCH_APPLICATION};

#[cfg(feature = "sparkle")]
const EVERY_RELEASE: &str = "https://releases.flashmind.com/beep/macos/appcast.xml";

#[derive(Default)]
pub struct Updates(AtomicBool);

/// Sparkle's relaunch must pass the app's window-close exit guard.
pub fn relaunching(app: &AppHandle) -> bool {
    app.state::<Updates>().0.load(Ordering::Acquire)
}

#[cfg(not(feature = "sparkle"))]
pub async fn follow(_app: AppHandle) {}

#[cfg(feature = "sparkle")]
pub async fn follow(app: AppHandle) {
    follow_feed(&app);

    let Some(updater) = app.sparkle_updater() else {
        return;
    };
    let handle = app.clone();
    updater.set_event_callback(Some(Arc::new(move |event, _| {
        if event == EVENT_WILL_RELAUNCH_APPLICATION {
            handle.state::<Updates>().0.store(true, Ordering::Release);
            let app = handle.clone();
            if let Err(error) =
                handle.run_on_main_thread(move || crate::store::window_session::checkpoint(&app))
            {
                log::warn!("saving windows before updating: {error}");
            }
        }
    })));
}

#[cfg(not(feature = "sparkle"))]
pub fn follow_feed(_app: &AppHandle) {}

/// Points Sparkle at the feed Developer mode picks.
#[cfg(feature = "sparkle")]
pub fn follow_feed(app: &AppHandle) {
    let Some(updater) = app.sparkle_updater() else {
        return;
    };
    let developer = app.state::<crate::beep::state::Prefs>().developer();

    if let Err(error) = updater.set_feed_url_override(developer.then(|| EVERY_RELEASE.to_owned())) {
        log::warn!("choosing the update feed: {error}");
    }
}

#[cfg(not(feature = "sparkle"))]
pub async fn check(app: &AppHandle, _asked: bool) {
    crate::notices::notify::text(app, "Updates require an installed Beep app", "");
}

#[cfg(feature = "sparkle")]
pub async fn check(app: &AppHandle, _asked: bool) {
    let Some(updater) = app.sparkle_updater() else {
        crate::notices::notify::text(app, "Updates require an installed Beep app", "");
        return;
    };

    if let Err(error) = updater.check_for_updates() {
        log::warn!("checking for an update: {error}");
        crate::notices::notify::text(app, "Could not check for updates", "Try again later.");
    }
}

// Sparkle presents pending downloads in its own dialog when checking again.
pub fn ready(_app: &AppHandle) -> Option<String> {
    None
}

pub fn install(_app: &AppHandle) {}

pub fn restart(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(&app, true).await });
}
