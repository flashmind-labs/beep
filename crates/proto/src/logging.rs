//! Logging for the node and server binaries: `log` and `tracing` events
//! (iroh logs with `tracing`) filtered by `RUST_LOG`, written as
//! `<time>   INFO <target>: <file>:<line> <message>` to stderr, to a daily
//! file, or both.

use anyhow::{Context, Result};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{Event, Subscriber};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{
    EnvFilter,
    fmt::{
        FmtContext, FormatEvent, FormatFields,
        format::Writer,
        time::{FormatTime, SystemTime as Utc},
    },
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
};

/// Starts logging with `default_filter` unless `RUST_LOG` is set: to stderr
/// when `stderr`, and to `file` when given.
pub fn start(default_filter: &str, stderr: bool, file: Option<RollingFile>) -> Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let stderr = stderr.then(|| {
        tracing_subscriber::fmt::layer()
            .event_format(TextLayout)
            .with_ansi(false)
            .with_writer(io::stderr)
    });
    let file = file.map(|file| {
        tracing_subscriber::fmt::layer()
            .event_format(TextLayout)
            .with_ansi(false)
            .with_writer(Arc::new(file))
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr)
        .with(file)
        .try_init()
        .context("starting the log")
}

/// `2026-11-03T14:30:59.238909Z   INFO beep_node::link: link.rs:10 connected`.
struct TextLayout;

impl<S, N> FormatEvent<S, N> for TextLayout
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        // Events from `log` carry their real target and file this way.
        let normalized = event.normalized_metadata();
        let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
        Utc.format_time(&mut writer)?;
        let level = meta.level();
        write!(writer, " {level:>6}")?;
        let file = meta
            .file()
            .map(|file| {
                Path::new(file)
                    .file_name()
                    .map_or(file.into(), |name| name.to_string_lossy())
            })
            .unwrap_or_default();
        let line = meta.line().unwrap_or_default();
        write!(writer, " {}: {file}:{line} ", meta.target())?;
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// A log file that starts afresh each day (UTC): `beep.log` is moved to
/// `beep.<date>.log`, and only the newest `keep` of those stay.
pub struct RollingFile {
    current: PathBuf,
    keep: usize,
    open: Mutex<Open>,
}

struct Open {
    file: File,
    day: i64,
}

impl RollingFile {
    /// Opens `path` to append to, first moving it aside if it was last
    /// written on an earlier day.
    pub fn new(path: &Path, keep: usize) -> Result<Self> {
        if let Some(folder) = path
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
        {
            fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
        }
        let today = day(SystemTime::now());
        let mut rolling = Self {
            current: path.to_owned(),
            keep,
            open: Mutex::new(Open {
                file: append(path)?,
                day: today,
            }),
        };
        let written = fs::metadata(path)
            .ok()
            .filter(|metadata| metadata.len() > 0)
            .and_then(|metadata| metadata.modified().ok())
            .map(day);
        if let Some(written) = written.filter(|&written| written < today) {
            let file = rolling.roll(written)?;
            match rolling.open.get_mut() {
                Ok(open) => open.file = file,
                Err(poisoned) => poisoned.into_inner().file = file,
            }
        }
        Ok(rolling)
    }

    /// Moves the current file to its dated name, removes the oldest dated
    /// files beyond `keep` and opens a new current file.
    fn roll(&self, day: i64) -> Result<File> {
        let (stem, suffix) = self.stem_and_suffix();
        let folder = self.folder();
        let date = date(day);
        let dated = (0..)
            .map(|n| match n {
                0 => folder.join(format!("{stem}.{date}{suffix}")),
                n => folder.join(format!("{stem}.{date}.{n}{suffix}")),
            })
            .find(|path| !path.exists())
            .context("no free name for the old log")?;
        fs::rename(&self.current, &dated)
            .with_context(|| format!("moving {} aside", self.current.display()))?;
        self.prune(&stem, &suffix)?;
        append(&self.current)
    }

    /// Removes the oldest dated files beyond `keep`.
    fn prune(&self, stem: &str, suffix: &str) -> Result<()> {
        let folder = self.folder();
        let prefix = format!("{stem}.");
        let mut dated: Vec<(SystemTime, PathBuf)> = fs::read_dir(&folder)
            .with_context(|| format!("reading {}", folder.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&prefix) && name.ends_with(suffix) && entry.path() != self.current
            })
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        dated.sort();
        let extra = dated.len().saturating_sub(self.keep);
        for (_, path) in dated.into_iter().take(extra) {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
        Ok(())
    }

    fn folder(&self) -> PathBuf {
        match self.current.parent() {
            Some(folder) if !folder.as_os_str().is_empty() => folder.to_owned(),
            _ => PathBuf::from("."),
        }
    }

    /// `beep` and `.log` for `beep.log`.
    fn stem_and_suffix(&self) -> (String, String) {
        let stem = self
            .current
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let suffix = self
            .current
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        (stem, suffix)
    }
}

impl Write for &RollingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut open = self
            .open
            .lock()
            .map_err(|_| io::Error::other("the log lock is poisoned"))?;
        let today = day(SystemTime::now());
        if today > open.day {
            match self.roll(open.day) {
                Ok(file) => open.file = file,
                // Keep writing to the old file rather than lose lines.
                Err(e) => eprintln!("rolling the log over: {e:#}"),
            }
            open.day = today;
        }
        open.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut open = self
            .open
            .lock()
            .map_err(|_| io::Error::other("the log lock is poisoned"))?;
        open.file.flush()
    }
}

fn append(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening the log {}", path.display()))
}

/// Days since 1970-01-01, UTC.
fn day(time: SystemTime) -> i64 {
    let seconds = time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    });
    seconds.div_euclid(86_400)
}

/// `2026-11-03` for a day since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn date(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dates_are_civil_dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(59), "1970-03-01");
        assert_eq!(date(20_760), "2026-11-03");
        assert_eq!(date(11_016), "2000-02-29");
    }

    #[test]
    fn a_file_from_an_earlier_day_is_moved_aside_and_old_ones_pruned() {
        let dir = std::env::temp_dir().join(format!("beep-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let current = dir.join("beep.log");
        let two_days_ago = SystemTime::now() - Duration::from_secs(2 * 86_400);
        for n in 0..3 {
            let old = dir.join(format!("beep.2020-01-0{}.log", n + 1));
            fs::write(&old, "old\n").unwrap();
            File::options()
                .write(true)
                .open(&old)
                .unwrap()
                .set_modified(two_days_ago - Duration::from_secs(60 * (3 - n)))
                .unwrap();
        }
        fs::write(&current, "yesterday\n").unwrap();
        File::options()
            .write(true)
            .open(&current)
            .unwrap()
            .set_modified(two_days_ago)
            .unwrap();

        let rolling = RollingFile::new(&current, 2).unwrap();
        (&rolling).write_all(b"today\n").unwrap();

        assert_eq!(fs::read_to_string(&current).unwrap(), "today\n");
        let moved = dir.join(format!("beep.{}.log", date(day(two_days_ago))));
        assert_eq!(fs::read_to_string(&moved).unwrap(), "yesterday\n");
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(
            names.contains(&"beep.2020-01-03.log".to_owned()),
            "{names:?}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
