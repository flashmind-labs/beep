//! The app's log, under `~/Library/Logs/Beep`, the place macOS keeps logs
//! and Console shows them, so a user can send it. Beep's own lines from
//! debug up but the per-snapshot event lines, the embedded node's from info
//! but its per-read subscription lines, iroh and the networking crates
//! under it only when something goes wrong. `RUST_LOG` overrides it.

use std::path::{Path, PathBuf};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{InitError, RollingFileAppender, Rotation},
};
use tracing_subscriber::EnvFilter;

const LOG_FILTER: &str = "info,beep_desktop=debug,beep_desktop_lib::fleet::events=info,\
                          beep_node::link::events=warn,iroh=warn,noq=warn,n0=warn,\
                          netwatch=warn,netdev=warn,portmapper=warn,igd_next=warn,\
                          notify::inotify=warn";
/// The current daily log and the two previous daily logs.
const FILES_KEPT: usize = 3;
/// A page's error is cut to this many characters.
const WEBVIEW_CHARS: usize = 4000;

pub(crate) fn dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Beep/Logs"))
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Logs/Beep"))
    }
}

/// Starts logging to the file, written on a thread of its own. The guard
/// flushes it when dropped. Without a file the app runs as before, silent.
pub fn start() -> Option<WorkerGuard> {
    let dir = dir()?;
    let file = match appender(&dir) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("no log file in {}: {e}", dir.display());
            return None;
        }
    };
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(LOG_FILTER));
    // Also takes the `log` crate's lines, which Beep and the node write.
    if let Err(e) = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
    {
        eprintln!("no log: {e}");
    }
    // A command that panics never answers its window: the log says so.
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default(info);
    }));
    Some(guard)
}

/// Logs a page's error or diagnostic. Existing callers send errors.
#[tauri::command]
pub fn log_webview(window: tauri::Window, message: String, info: Option<bool>) {
    let message: String = message.chars().take(WEBVIEW_CHARS).collect();
    if info.unwrap_or(false) {
        log::info!(target: "webview", "{}: {message}", window.label());
    } else {
        log::error!(target: "webview", "{}: {message}", window.label());
    }
}

fn appender(dir: &Path) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("beep")
        .filename_suffix("log")
        .max_log_files(FILES_KEPT)
        .build(dir)
}

#[cfg(test)]
#[path = "../tests/platform_logfile.rs"]
mod tests;
