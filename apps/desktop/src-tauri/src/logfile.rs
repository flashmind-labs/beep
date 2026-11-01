//! The app's log, under `~/Library/Logs/Beep`, the place macOS keeps logs
//! and Console shows them. One file a day: Beep's own lines from debug up,
//! everything else from warnings. `RUST_LOG` overrides it.

use logforth::{append::file::FileBuilder, filter::rustlog::RustLogFilterBuilder};
use std::{num::NonZeroUsize, path::PathBuf};

/// Daily log files kept before the oldest is deleted.
const LOG_FILES_KEPT: NonZeroUsize = NonZeroUsize::new(7).expect("the count is not zero");

pub(crate) fn dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Logs/Beep"))
}

/// Starts logging to the file. Without one the app runs as before, silent.
pub fn start() {
    let Some(dir) = dir() else { return };
    let file = match FileBuilder::new(&dir, "beep")
        .filename_suffix("log")
        .rollover_daily()
        .max_log_files(LOG_FILES_KEPT)
        .build()
    {
        Ok(file) => file,
        Err(e) => {
            eprintln!("no log file in {}: {e}", dir.display());
            return;
        }
    };
    logforth::starter_log::builder()
        .dispatch(|d| {
            d.filter(RustLogFilterBuilder::from_default_env_or("warn,beep_desktop=debug").build())
                .append(file)
        })
        .apply();
}
