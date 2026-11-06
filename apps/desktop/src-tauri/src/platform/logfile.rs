//! The app's log, under `~/Library/Logs/Beep`, the place macOS keeps logs
//! and Console shows them, so a user can send it. Beep's own lines from
//! debug up, the embedded node's from info, iroh and the networking crates
//! under it only when something goes wrong. `RUST_LOG` overrides it.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

const LOG_FILTER: &str = "info,beep_desktop=debug,iroh=warn,noq=warn,n0=warn,netwatch=warn,\
                          netdev=warn,portmapper=warn,igd_next=warn";
/// A file is closed and a new one started past this size.
const FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Files kept: `beep.log`, then `beep.1.log` and `beep.2.log`, the oldest.
const FILES_KEPT: usize = 3;
/// A page's error is cut to this many characters.
const WEBVIEW_CHARS: usize = 4000;

pub(crate) fn dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Logs/Beep"))
}

/// The file being written now.
pub(crate) fn current() -> Option<PathBuf> {
    Some(path(&dir()?, 0))
}

/// The log files there may be, oldest first, the one being written last.
/// One not written yet is skipped when read.
pub(crate) fn kept() -> Vec<PathBuf> {
    let Some(dir) = dir() else {
        return Vec::new();
    };
    (0..FILES_KEPT).rev().map(|n| path(&dir, n)).collect()
}

/// Starts logging to the file, written on a thread of its own. The guard
/// flushes it when dropped. Without a file the app runs as before, silent.
pub fn start() -> Option<WorkerGuard> {
    let dir = dir()?;
    let file = match Rolling::open(&dir, FILE_BYTES) {
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
    Some(guard)
}

/// Logs an error from a page: an uncaught error, a rejected promise or a
/// failed command.
#[tauri::command]
pub fn log_webview(window: tauri::Window, message: String) {
    let message: String = message.chars().take(WEBVIEW_CHARS).collect();
    log::error!(target: "webview", "{}: {message}", window.label());
}

/// A log file that moves aside once it reaches `max` bytes, keeping
/// [`FILES_KEPT`] files in all.
struct Rolling {
    dir: PathBuf,
    file: File,
    size: u64,
    max: u64,
}

impl Rolling {
    fn open(dir: &Path, max: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path(dir, 0))?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_owned(),
            file,
            size,
            max,
        })
    }

    fn roll(&mut self) -> io::Result<()> {
        for n in (1..FILES_KEPT).rev() {
            match fs::rename(path(&self.dir, n - 1), path(&self.dir, n)) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        self.file = File::create(path(&self.dir, 0))?;
        self.size = 0;
        Ok(())
    }
}

impl Write for Rolling {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.size > 0 && self.size + buf.len() as u64 > self.max {
            self.roll()?;
        }
        let n = self.file.write(buf)?;
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn path(dir: &Path, n: usize) -> PathBuf {
    match n {
        0 => dir.join("beep.log"),
        n => dir.join(format!("beep.{n}.log")),
    }
}

#[cfg(test)]
mod tests {
    use super::{FILES_KEPT, Rolling, path};
    use std::{env, fs, io::Write};
    use uuid::Uuid;

    #[test]
    fn rolling_keeps_three_files_under_the_size() {
        let dir = env::temp_dir().join(format!("beep-log-{}", Uuid::new_v4()));
        let mut log = Rolling::open(&dir, 10).expect("the temp dir is writable");
        for line in ["aaaaaa\n", "bbbbbb\n", "cccccc\n", "dddddd\n"] {
            log.write_all(line.as_bytes())
                .expect("the file is writable");
        }
        let read = |n| fs::read_to_string(path(&dir, n)).expect("the file was written");
        assert_eq!(read(0), "dddddd\n");
        assert_eq!(read(1), "cccccc\n");
        assert_eq!(read(2), "bbbbbb\n");
        assert!(!path(&dir, FILES_KEPT).exists());
        fs::remove_dir_all(&dir).expect("the temp dir is removable");
    }
}
