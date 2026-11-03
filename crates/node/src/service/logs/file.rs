//! The log file the service writes: the LaunchAgent's output on macOS, the
//! node's daily files on Windows. Followed by polling it for growth.

use super::{LogOptions, Printer, cutoff};
use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

/// How often a followed file is checked for new lines.
const POLL: Duration = Duration::from_millis(500);

/// The files to read: older ones first, then the one being written.
struct LogFiles {
    older: Vec<PathBuf>,
    current: PathBuf,
}

#[cfg(target_os = "macos")]
fn files(_cutoff: i64) -> Result<LogFiles> {
    use crate::service::launchd::{log_path, plist_path};
    let plist = plist_path()?;
    if !plist.exists() {
        bail!(
            "the node's service is not installed here: there is no {}",
            plist.display()
        );
    }
    Ok(LogFiles {
        older: Vec::new(),
        current: log_path()?,
    })
}

#[cfg(windows)]
fn files(cutoff: i64) -> Result<LogFiles> {
    use crate::service::windows::{LOG_NAME, installed, log_dir};
    if !installed() {
        bail!("the node's service is not installed here: there is no scheduled task for it");
    }
    let dir = log_dir()?;
    let stem = LOG_NAME.trim_end_matches(".log");
    let entries = fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
    // Rolled over files are `beep-node.<date>.<n>.log`; ones last written
    // before the cutoff hold nothing to show.
    let mut older: Vec<(i64, PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let rolled =
                name != LOG_NAME && name.starts_with(&format!("{stem}.")) && name.ends_with(".log");
            let modified = modified(&entry.metadata().ok()?)?;
            (rolled && modified >= cutoff).then(|| (modified, entry.path()))
        })
        .collect();
    older.sort();
    Ok(LogFiles {
        older: older.into_iter().map(|(_, path)| path).collect(),
        current: dir.join(LOG_NAME),
    })
}

/// When a file was last written, in Unix seconds.
#[cfg(windows)]
fn modified(metadata: &fs::Metadata) -> Option<i64> {
    use std::time::UNIX_EPOCH;
    let since = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

pub(super) fn read(options: &LogOptions, printer: &mut Printer<impl Write>) -> Result<()> {
    let files = files(cutoff(options.since))?;
    for path in &files.older {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut pending = Vec::new();
        drain(&mut BufReader::new(file), &mut pending, printer)?;
        if !pending.is_empty() {
            printer.bytes(&pending)?;
        }
    }
    let current = &files.current;
    let file = File::open(current).with_context(|| format!("opening {}", current.display()))?;
    let mut reader = BufReader::new(file);
    let mut pending = Vec::new();
    let mut read = drain(&mut reader, &mut pending, printer)?;
    if !options.follow {
        if !pending.is_empty() {
            printer.bytes(&pending)?;
        }
        printer.caught_up()?;
        return Ok(());
    }
    printer.caught_up()?;
    loop {
        thread::sleep(POLL);
        let len = match fs::metadata(current) {
            Ok(metadata) => metadata.len(),
            // Between a rollover's rename and the new file.
            Err(_) => continue,
        };
        if len < read {
            // Rolled over or truncated: the file now holds only new lines.
            let file =
                File::open(current).with_context(|| format!("opening {}", current.display()))?;
            reader = BufReader::new(file);
            pending.clear();
            read = 0;
        } else if len == read {
            continue;
        }
        reader.seek(SeekFrom::Start(read))?;
        read += drain(&mut reader, &mut pending, printer)?;
        printer.out.flush()?;
    }
}

/// Passes each whole line `reader` has left to `printer`, leaving a last
/// unfinished one in `pending`; returns the bytes read.
fn drain(
    reader: &mut impl BufRead,
    pending: &mut Vec<u8>,
    printer: &mut Printer<impl Write>,
) -> Result<u64> {
    let mut read = 0;
    loop {
        let n = reader.read_until(b'\n', pending)?;
        if n == 0 {
            return Ok(read);
        }
        read += u64::try_from(n).context("a line longer than memory")?;
        if pending.ends_with(b"\n") {
            printer.bytes(pending)?;
            pending.clear();
        }
    }
}
