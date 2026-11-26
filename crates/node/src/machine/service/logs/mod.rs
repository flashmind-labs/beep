//! `beep-node logs`: the service's log, read from wherever `setup` put it
//! (the journal under systemd, a file under OpenRC, SysV init, macOS and
//! Windows), without the
//! networking crates' chatter unless asked.

use anyhow::{Context, Result};
use beep_proto::node::logs::{MAX_BYTES, MAX_LINES, Query};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset};
use regex::{Regex, RegexBuilder};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, ErrorKind, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod file;
#[cfg(target_os = "linux")]
mod journal;
mod print;
mod serve;

use print::{Filter, Printer, Tail, cutoff, head, is_noise, plain};

pub use print::LogOptions;
pub use serve::Logs;

/// The most text one page of a log query holds.
const PAGE_BYTES: usize = 1024 * 1024;
/// How far back `recent` looks.
const RECENT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Prints the service's log as `options` asks.
pub fn show(options: &LogOptions) -> Result<()> {
    let stdout = io::stdout().lock();
    let mut printer = Printer::new(options, cutoff(options.since), stdout);

    match read(options, &mut printer) {
        // `beep-node logs | head` closed the pipe: nothing more to show.
        Err(e) if is_broken_pipe(&e) => Ok(()),
        other => other,
    }
}

/// The last `lines` lines of the service's log, or of `files` oldest first,
/// cut to `MAX_LINES` and `MAX_BYTES`.
pub(crate) fn recent(files: &[PathBuf], lines: u32) -> Result<String> {
    let options = LogOptions {
        since: RECENT,
        follow: false,
        lines: Some(usize::try_from(lines.min(MAX_LINES))?),
        all: false,
    };
    let mut printer = Printer::new(&options, cutoff(RECENT), Vec::new());

    read_files(files, &options, None, &mut printer)?;

    if !files.is_empty() {
        printer.caught_up()?;
    }

    let out = &printer.out;
    let from = out.len().saturating_sub(MAX_BYTES);
    let whole = match out[from..].iter().position(|&byte| byte == b'\n') {
        Some(newline) if from > 0 => from + newline + 1,
        _ => from,
    };
    Ok(String::from_utf8_lossy(&out[whole..]).into_owned())
}

/// The log from Unix second `since` on, in whole lines, the newest kept up
/// to `max_bytes`: of the log files `files`, oldest first, or of the
/// service's log when there are none. A log with no timestamp at all gives
/// its last lines.
pub(crate) fn window(files: &[PathBuf], since: i64, max_bytes: usize) -> Result<String> {
    // A long journal takes seconds to read, for no bytes to keep.
    if max_bytes == 0 {
        return Ok(String::new());
    }
    let read = window_from(files, Some(since), max_bytes)?;

    if read.timed {
        return Ok(read.text);
    }

    Ok(window_from(files, None, max_bytes)?.text)
}

/// Filters while scanning, before retaining a bounded page of matching lines.
fn query(files: &[PathBuf], query: &Query) -> Result<String> {
    anyhow::ensure!(
        (1..=2000).contains(&query.lines),
        "lines must be between 1 and 2000"
    );
    anyhow::ensure!(query.offset <= 100_000, "offset must be at most 100000");
    anyhow::ensure!(
        !query.regex || query.search.is_some(),
        "regex requires search"
    );
    let start = DateTime::parse_from_rfc3339(&query.start)?;
    let end = DateTime::parse_from_rfc3339(&query.end)?;
    anyhow::ensure!(
        start.offset().local_minus_utc() == 0 && end.offset().local_minus_utc() == 0,
        "start and end must be UTC"
    );
    anyhow::ensure!(
        start < end && end.signed_duration_since(start) <= ChronoDuration::days(7),
        "start must precede end by at most 7 days"
    );
    let pattern: Option<Regex> = query
        .search
        .as_ref()
        .map(|search| {
            anyhow::ensure!(search.len() <= 4096, "search must be at most 4096 bytes");
            let pattern = if query.regex {
                search.clone()
            } else {
                regex::escape(search)
            };
            Ok::<_, anyhow::Error>(
                RegexBuilder::new(&pattern)
                    .size_limit(1024 * 1024)
                    .build()?,
            )
        })
        .transpose()?;
    let mut sink = QueryWriter {
        start,
        end,
        current: None,
        allow_untimed: query.allow_untimed,
        pattern,
        pending: Vec::new(),
        scanned: 0,
        tail: Tail::new((query.lines + query.offset) as usize),
    };
    let options = LogOptions {
        since: Duration::from_secs(
            cutoff(Duration::ZERO)
                .saturating_sub(start.timestamp())
                .max(0) as u64,
        ),
        follow: false,
        lines: None,
        all: false,
    };
    let mut printer = Printer {
        filter: Filter::new(true, None),
        tail: None,
        out: &mut sink,
    };

    read_files(files, &options, Some(start.timestamp()), &mut printer)?;

    let keep = sink.tail.lines.len().saturating_sub(query.offset as usize);
    sink.tail.lines.truncate(keep);
    let bytes: usize = sink.tail.lines.iter().map(|line| line.len() + 1).sum();

    anyhow::ensure!(
        bytes <= PAGE_BYTES,
        "Page exceeds 1 MiB; request fewer lines"
    );
    Ok(sink
        .tail
        .lines
        .into_iter()
        .map(|line| line + "\n")
        .collect())
}

struct QueryWriter {
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
    current: Option<DateTime<FixedOffset>>,
    allow_untimed: bool,
    pattern: Option<Regex>,
    pending: Vec<u8>,
    scanned: usize,
    tail: Tail,
}

impl Write for QueryWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.scanned = self.scanned.saturating_add(bytes.len());

        if self.scanned > 256 * 1024 * 1024 {
            return Err(io::Error::other("Log query scan exceeds 256 MiB"));
        }

        for &byte in bytes {
            if byte != b'\n' {
                if self.pending.len() >= 1024 * 1024 {
                    return Err(io::Error::other("Log line exceeds 1 MiB"));
                }
                self.pending.push(byte);
                continue;
            }

            let text = String::from_utf8_lossy(&self.pending);
            let line = plain(&text);
            if let Some(time) = line
                .split_whitespace()
                .next()
                .and_then(|word| DateTime::parse_from_rfc3339(word).ok())
            {
                self.current = Some(time);
            }
            let in_window = self.current.map_or(self.allow_untimed, |time| {
                time >= self.start && time < self.end
            });

            let noisy = head(&line).is_some_and(|head| is_noise(&head));

            if in_window
                && !noisy
                && self
                    .pattern
                    .as_ref()
                    .is_none_or(|pattern| pattern.is_match(&line))
            {
                self.tail.push(&line);

                if self.tail.bytes > 16 * 1024 * 1024 {
                    return Err(io::Error::other(
                        "Log pagination exceeds 16 MiB; narrow the time range or search",
                    ));
                }
            }
            self.pending.clear();
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// What `window_from` read.
struct Window {
    text: String,
    /// Whether any line had a timestamp.
    timed: bool,
}

fn window_from(files: &[PathBuf], cutoff: Option<i64>, max_bytes: usize) -> Result<Window> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |now| now.as_secs());
    // No cutoff, or one before 1970, is the whole log.
    let from = cutoff.unwrap_or(0).max(0).unsigned_abs();
    let options = LogOptions {
        since: Duration::from_secs(now.saturating_sub(from)),
        follow: false,
        lines: None,
        all: false,
    };
    let mut printer = Printer {
        filter: Filter::new(false, cutoff),
        tail: Some(Tail::bytes(max_bytes)),
        out: Vec::new(),
    };
    if files.is_empty() {
        read(&options, &mut printer)?;
    } else {
        for path in files {
            // A file not written yet, or rolled away mid-read, holds nothing here.
            let file = match File::open(path) {
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                file => file.with_context(|| format!("opening {}", path.display()))?,
            };

            for line in BufReader::new(file).split(b'\n') {
                printer.bytes(&line?)?;
            }
        }

        printer.caught_up()?;
    }

    Ok(Window {
        text: String::from_utf8_lossy(&printer.out).into_owned(),
        timed: printer.filter.timed,
    })
}

/// Reads `files` oldest first, those not there yet or rolled away mid-read
/// holding nothing, or the service's log when there are none.
fn read_files(
    files: &[PathBuf],
    options: &LogOptions,
    start: Option<i64>,
    printer: &mut Printer<impl Write>,
) -> Result<()> {
    if files.is_empty() {
        return read_from(options, printer, start);
    }

    for file in files {
        match read_file(file, printer) {
            Err(e)
                if e.downcast_ref::<io::Error>()
                    .is_some_and(|e| e.kind() == ErrorKind::NotFound) => {}
            result => result?,
        }
    }

    Ok(())
}

fn read_file(path: &Path, printer: &mut Printer<impl Write>) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;

    for line in BufReader::new(file).split(b'\n') {
        printer.bytes(&line?)?;
    }

    Ok(())
}

fn read(options: &LogOptions, printer: &mut Printer<impl Write>) -> Result<()> {
    read_from(options, printer, None)
}

#[cfg(target_os = "linux")]
fn read_from(
    options: &LogOptions,
    printer: &mut Printer<impl Write>,
    start: Option<i64>,
) -> Result<()> {
    use super::{Manager, systemd::Scope};

    match super::present()? {
        Some(Manager::Systemd) => journal::read(options, printer, Scope::User, start),
        Some(Manager::SystemdSystem) => journal::read(options, printer, Scope::System, start),
        Some(_) => file::read(options, printer),
        None => anyhow::bail!("the node's service is not installed here"),
    }
}

#[cfg(any(target_os = "macos", windows))]
fn read_from(
    options: &LogOptions,
    printer: &mut Printer<impl Write>,
    _start: Option<i64>,
) -> Result<()> {
    file::read(options, printer)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn read_from(
    _options: &LogOptions,
    _printer: &mut Printer<impl Write>,
    _start: Option<i64>,
) -> Result<()> {
    anyhow::bail!(super::Unusable::Os(std::env::consts::OS))
}

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == ErrorKind::BrokenPipe)
}

/// Reads `--since`: a whole number with `s`, `m`, `h` or `d`, such as `1h`.
pub fn parse_since(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let wrong = || format!("{text:?} is not a time span; use one like 45s, 30m, 1h or 2d");
    let split = text.find(|c: char| !c.is_ascii_digit()).ok_or_else(wrong)?;
    let (number, unit) = text.split_at(split);
    let number: u64 = number.parse().map_err(|_| wrong())?;
    let seconds: u64 = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err(wrong()),
    };
    number
        .checked_mul(seconds)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("{text} is too long ago"))
}

#[cfg(test)]
#[path = "../../../tests/machine_service_logs.rs"]
mod tests;
