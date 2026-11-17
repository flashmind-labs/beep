//! The app's log, under `~/Library/Logs/Beep` on macOS (`Beep Dev` for a
//! debug build; Console shows it),
//! the XDG state folder on Linux and the app's data folder on Android
//! (also on logcat), so a user can send it. Beep's own lines from
//! debug up but the per-snapshot event lines, the embedded node's from info
//! but its per-read subscription lines, plugins' own lines (target `plugin`,
//! `<id>: text`) from info, iroh and the networking crates
//! under it only when something goes wrong. `RUST_LOG` overrides it.

use std::path::{Path, PathBuf};
#[cfg(target_os = "android")]
use std::sync::OnceLock;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{InitError, RollingFileAppender, Rotation},
};
#[cfg(target_os = "android")]
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

const LOG_FILTER: &str = "info,beep_desktop=debug,beep_desktop_lib::fleet::events=info,\
                          beep_node::link::events=warn,plugin=info,iroh=warn,iroh::protocol=info,noq=error,n0=warn,\
                          netwatch=error,netdev=warn,portmapper=warn,igd_next=warn,\
                          notify::inotify=warn";
/// The current daily log and the two previous daily logs.
const FILES_KEPT: usize = 3;
/// A page's error is cut to this many characters.
const WEBVIEW_CHARS: usize = 4000;

/// Android's log folder, known once Tauri gives the app's data folder.
#[cfg(target_os = "android")]
static ANDROID_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The log folder's name: a debug build (`just run`) logs apart from an
/// installed Beep.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const NAME: &str = if cfg!(debug_assertions) {
    "Beep Dev"
} else {
    "Beep"
};

pub(crate) fn dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        ANDROID_DIR.get().cloned()
    }
    #[cfg(windows)]
    {
        Some(
            PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
                .join(NAME)
                .join("Logs"),
        )
    }
    #[cfg(target_os = "linux")]
    {
        linux_dir(dirs::state_dir(), dirs::data_dir())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Logs").join(NAME))
    }
}

#[cfg(target_os = "linux")]
fn linux_dir(state: Option<PathBuf>, data: Option<PathBuf>) -> Option<PathBuf> {
    Some(state.or(data)?.join("beep/logs"))
}

/// Starts logging to the file, written on a thread of its own. The guard
/// flushes it when dropped. Without a file the app runs as before, silent.
/// Android starts it later, in `start_android`.
pub fn start() -> Option<WorkerGuard> {
    #[cfg(target_os = "android")]
    {
        None
    }
    #[cfg(not(target_os = "android"))]
    start_file(&dir()?)
}

/// Starts Android's log under the app's `data` folder, once Tauri gives it.
#[cfg(target_os = "android")]
pub fn start_android(data: &Path) -> Option<WorkerGuard> {
    let dir = ANDROID_DIR.get_or_init(|| data.join("logs"));
    start_file(dir)
}

fn start_file(dir: &Path) -> Option<WorkerGuard> {
    let file = match appender(dir) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("no log file in {}: {e}", dir.display());
            #[cfg(target_os = "android")]
            init(std::io::stderr);
            return None;
        }
    };
    let (writer, guard) = tracing_appender::non_blocking(file);
    // Logcat keeps every line too.
    #[cfg(target_os = "android")]
    let writer = writer.and(std::io::stderr);
    init(writer);
    Some(guard)
}

fn init<W>(writer: W)
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
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
