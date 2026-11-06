//! `beep-node logs`: the service's log, read from wherever `setup` put it
//! (the journal on Linux, a file on macOS and Windows), without the
//! networking crates' chatter unless asked.

use anyhow::{Context, Result};
use beep_proto::node::logs::{MAX_BYTES, MAX_LINES};
use std::{
    borrow::Cow,
    collections::VecDeque,
    fs::File,
    io::{self, BufRead, BufReader, ErrorKind, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "macos", windows))]
mod file;
#[cfg(target_os = "linux")]
mod journal;
mod serve;

pub use serve::Logs;

/// How far back `recent` looks.
const RECENT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// What `beep-node logs` shows.
#[derive(Debug, Clone, Copy)]
pub struct LogOptions {
    /// How far back to start.
    pub since: Duration,
    /// Keeps printing lines as they are written.
    pub follow: bool,
    /// Only the last this many lines of the past, after filtering.
    pub lines: Option<usize>,
    /// Shows every line, the networking crates' info lines too.
    pub all: bool,
}

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

/// The last `lines` lines of the service's log, or of the log file `file`,
/// cut to `MAX_LINES` and `MAX_BYTES`.
pub(crate) fn recent(file: Option<&Path>, lines: u32) -> Result<String> {
    let options = LogOptions {
        since: RECENT,
        follow: false,
        lines: Some(usize::try_from(lines.min(MAX_LINES))?),
        all: false,
    };
    let mut printer = Printer::new(&options, cutoff(RECENT), Vec::new());
    match file {
        Some(file) => {
            read_file(file, &mut printer)?;
            printer.caught_up()?;
        }
        None => read(&options, &mut printer)?,
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
    let read = window_from(files, Some(since), max_bytes)?;
    if read.timed {
        return Ok(read.text);
    }
    Ok(window_from(files, None, max_bytes)?.text)
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
        for file in files {
            read_file(file, &mut printer)?;
        }
        printer.caught_up()?;
    }
    Ok(Window {
        text: String::from_utf8_lossy(&printer.out).into_owned(),
        timed: printer.filter.timed,
    })
}

fn read_file(path: &Path, printer: &mut Printer<impl Write>) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    for line in BufReader::new(file).split(b'\n') {
        printer.bytes(&line?)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read(options: &LogOptions, printer: &mut Printer<impl Write>) -> Result<()> {
    journal::read(options, printer)
}

#[cfg(any(target_os = "macos", windows))]
fn read(options: &LogOptions, printer: &mut Printer<impl Write>) -> Result<()> {
    file::read(options, printer)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn read(_options: &LogOptions, _printer: &mut Printer<impl Write>) -> Result<()> {
    anyhow::bail!(super::Unusable::Os(std::env::consts::OS))
}

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == ErrorKind::BrokenPipe)
}

/// The earliest second shown, in Unix time.
fn cutoff(since: Duration) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |now| now.as_secs());
    let since = since.as_secs();
    i64::try_from(now.saturating_sub(since)).unwrap_or(i64::MAX)
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

/// Crates whose info lines are noise: iroh logs every packet path, and the
/// crates under it (QUIC, network watching, port mapping) are as chatty.
const NOISY_CRATES: &[&str] = &[
    "iroh",
    "noq",
    "n0",
    "netwatch",
    "netdev",
    "portmapper",
    "igd_next",
];

/// Whether `target` is a crate in `NOISY_CRATES`, one of its modules, or a
/// sibling crate such as `iroh_relay`; also iroh's spans, which the log
/// bridge prints as `tracing::span`.
fn noisy_target(target: &str) -> bool {
    if target == "tracing::span" || target.starts_with("tracing::span::") {
        return true;
    }
    let root = target.split("::").next().unwrap_or(target);
    NOISY_CRATES.iter().any(|name| {
        root.strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
    })
}

/// The start of a node log line: `<timestamp> <LEVEL> <target>: ...`.
#[derive(Debug, PartialEq, Eq)]
struct Head<'a> {
    /// Unix seconds.
    time: i64,
    level: &'a str,
    target: &'a str,
}

/// The head of `line`, which has had its colors removed; `None` for a line
/// that does not start with a timestamp, such as a panic or a message from
/// the service manager.
fn head(line: &str) -> Option<Head<'_>> {
    let mut words = line.split_whitespace();
    let time = parse_time(words.next()?)?;
    let level = words.next().unwrap_or("");
    let target = words.next().unwrap_or("");
    let target = target.strip_suffix(':').unwrap_or(target);
    Some(Head {
        time,
        level,
        target,
    })
}

/// Whether a line with this head is chatter to hide.
fn is_noise(head: &Head<'_>) -> bool {
    matches!(head.level, "INFO" | "DEBUG" | "TRACE") && noisy_target(head.target)
}

/// `line` without ANSI color codes, which the node writes when its stderr
/// is a file (macOS).
fn plain(line: &str) -> Cow<'_, str> {
    if !line.contains('\x1b') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        // `ESC [ params letter`: skip up to and with the final letter.
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    Cow::Owned(out)
}

/// Parses an RFC 3339 timestamp with an offset, as the node's logger writes
/// it (`2026-01-02T03:04:05.123456+01:00`), into Unix seconds.
fn parse_time(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let digits = text.get(from..to)?;
        if !digits.bytes().all(|d| d.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok()
    };
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    // Fractions of a second do not matter for a cutoff.
    let rest = text[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match rest.as_bytes() {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let minutes =
                num(text.len() - 5, text.len() - 3)? * 60 + num(text.len() - 2, text.len())?;
            if *sign == b'-' { -minutes } else { minutes }
        }
        _ => return None,
    };
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset * 60)
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar
/// (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Decides which lines to show.
#[derive(Debug)]
struct Filter {
    /// Shows the noise too.
    all: bool,
    /// Lines before this Unix second are left out; `None` keeps them all.
    cutoff: Option<i64>,
    /// Whether the last timestamped line was at or after the cutoff. Lines
    /// without a timestamp go with it.
    in_window: bool,
    /// Whether a line with a timestamp came.
    timed: bool,
}

impl Filter {
    fn new(all: bool, cutoff: Option<i64>) -> Self {
        Self {
            all,
            cutoff,
            in_window: cutoff.is_none(),
            timed: false,
        }
    }

    /// Whether to show `line`.
    fn keep(&mut self, line: &str) -> bool {
        let plain = plain(line);
        let Some(head) = head(&plain) else {
            return self.in_window;
        };
        self.timed = true;
        if !self.in_window {
            self.in_window = self.cutoff.is_none_or(|cutoff| head.time >= cutoff);
        }
        self.in_window && (self.all || !is_noise(&head))
    }
}

/// The last lines kept, up to a count and a size.
#[derive(Debug)]
struct Tail {
    cap: usize,
    /// The most bytes kept, newlines included.
    max_bytes: usize,
    bytes: usize,
    lines: VecDeque<String>,
}

impl Tail {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            max_bytes: usize::MAX,
            bytes: 0,
            lines: VecDeque::with_capacity(cap.min(4096)),
        }
    }

    /// As many of the last lines as fit in `max_bytes`.
    fn bytes(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            ..Self::new(usize::MAX)
        }
    }

    fn push(&mut self, line: &str) {
        if self.cap == 0 {
            return;
        }
        if self.lines.len() == self.cap {
            self.pop();
        }
        self.bytes += line.len() + 1;
        self.lines.push_back(line.to_owned());
        while self.bytes > self.max_bytes {
            self.pop();
        }
    }

    fn pop(&mut self) {
        if let Some(line) = self.lines.pop_front() {
            self.bytes -= line.len() + 1;
        }
    }
}

/// Writes the lines the filter keeps; the past ones through `Tail` when
/// only the last few are asked for.
struct Printer<W: Write> {
    filter: Filter,
    tail: Option<Tail>,
    out: W,
}

impl<W: Write> Printer<W> {
    fn new(options: &LogOptions, cutoff: i64, out: W) -> Self {
        Self {
            filter: Filter::new(options.all, Some(cutoff)),
            tail: options.lines.map(Tail::new),
            out,
        }
    }

    /// Takes one line, without its newline.
    fn line(&mut self, line: &str) -> io::Result<()> {
        if !self.filter.keep(line) {
            return Ok(());
        }
        match &mut self.tail {
            Some(tail) => {
                tail.push(line);
                Ok(())
            }
            None => writeln!(self.out, "{line}"),
        }
    }

    /// Takes one line as read, newline and all, which may not be UTF-8.
    fn bytes(&mut self, line: &[u8]) -> io::Result<()> {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        self.line(&String::from_utf8_lossy(line))
    }

    /// Ends the past: prints the tail kept, and from here on every line as
    /// it comes.
    fn caught_up(&mut self) -> io::Result<()> {
        if let Some(tail) = self.tail.take() {
            for line in tail.lines {
                writeln!(self.out, "{line}")?;
            }
        }
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOISE: &str = "2026-11-03T14:30:59.238909+01:00   INFO iroh::socket::transports: \
                         transports.rs:964 poll_send; len=42";
    const SPAN: &str = "2026-11-03T14:31:00.000001+01:00   INFO tracing::span: actor.rs:796 tx;";
    const NODE: &str = "2026-11-03T14:31:01.5+01:00   INFO beep_node::link: link.rs:10 connected";
    const IROH_WARN: &str = "2026-11-03T14:31:02.000000+01:00   WARN iroh::socket: lost the relay";

    fn options(lines: Option<usize>, all: bool) -> LogOptions {
        LogOptions {
            since: Duration::from_secs(3600),
            follow: false,
            lines,
            all,
        }
    }

    fn shown(options: &LogOptions, cutoff: i64, lines: &[&str]) -> Vec<String> {
        let mut printer = Printer::new(options, cutoff, Vec::new());
        for line in lines {
            printer.line(line).expect("writing to a Vec cannot fail");
        }
        printer.caught_up().expect("writing to a Vec cannot fail");
        String::from_utf8(printer.out)
            .expect("the lines written were UTF-8")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn timestamps_read_with_their_offset() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse_time("1969-12-31T23:00:00.5-01:00"), Some(0));
        assert_eq!(
            parse_time("2026-11-03T14:30:59.238909+01:00"),
            Some(1_793_712_659)
        );
        assert_eq!(parse_time("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        for wrong in [
            "Nov 03 14:30:59",
            "2026-11-03",
            "2026-13-03T00:00:00Z",
            "2026-11-03T14:30:59",
        ] {
            assert_eq!(parse_time(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn colors_do_not_hide_the_level() {
        let colored = "2026-11-03T14:30:59Z \x1b[32m INFO\x1b[0m iroh::net_report: report";
        assert_eq!(
            plain(colored),
            "2026-11-03T14:30:59Z  INFO iroh::net_report: report"
        );
        let mut filter = Filter::new(false, None);
        assert!(!filter.keep(colored));
    }

    #[test]
    fn the_default_hides_noise_and_keeps_everything_else() {
        let lines = [NOISE, SPAN, NODE, "thread 'main' panicked", IROH_WARN];
        let cutoff = head(NOISE).expect("a timestamped line").time;
        assert_eq!(
            shown(&options(None, false), cutoff, &lines),
            [NODE, "thread 'main' panicked", IROH_WARN]
        );
        assert_eq!(shown(&options(None, true), cutoff, &lines), lines);
    }

    #[test]
    fn lines_before_the_cutoff_are_left_out_with_their_continuations() {
        let old = "2026-11-03T13:00:00+01:00  WARN beep_node: old";
        let lines = [old, "  continued", NODE, "  continued too"];
        let cutoff = head(NOISE).expect("a timestamped line").time;
        assert_eq!(
            shown(&options(None, false), cutoff, &lines),
            [NODE, "  continued too"]
        );
    }

    #[test]
    fn after_catching_up_lines_print_as_they_come() {
        let mut printer = Printer::new(&options(Some(1), false), 0, Vec::new());
        printer.line(NODE).expect("writing to a Vec cannot fail");
        printer
            .line(IROH_WARN)
            .expect("writing to a Vec cannot fail");
        printer.caught_up().expect("writing to a Vec cannot fail");
        printer
            .bytes(format!("{NODE}\r\n").as_bytes())
            .expect("writing to a Vec cannot fail");
        let out = String::from_utf8(printer.out).expect("the lines written were UTF-8");
        assert_eq!(out, format!("{IROH_WARN}\n{NODE}\n"));
    }

    /// A node log line written `secs` after the Unix epoch.
    fn at(secs: i64, text: &str) -> String {
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        format!("1970-01-01T{h:02}:{m:02}:{s:02}.000001Z  INFO beep_node: {text}")
    }

    fn log_file(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
        let path = dir.join(name);
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&path, text).expect("the temp folder is writable");
        path
    }

    #[test]
    fn window_keeps_lines_since_and_their_untimed_followers() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let trace = "   0: beep_node::link::connect".to_owned();
        let lines = [
            at(100, "old"),
            at(200, "kept"),
            trace.clone(),
            at(300, "new"),
        ];
        let file = log_file(dir.path(), "beep.log", &lines);
        let text = window(&[file], 150, 1 << 20).expect("the file reads");
        assert_eq!(text, format!("{}\n{trace}\n{}\n", lines[1], lines[3]));
    }

    #[test]
    fn window_reads_rotated_files_oldest_first() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let old = log_file(dir.path(), "beep.1.log", &[at(100, "a"), at(200, "b")]);
        let new = log_file(dir.path(), "beep.log", &[at(300, "c")]);
        let text = window(&[old, new], 150, 1 << 20).expect("the files read");
        assert_eq!(text, format!("{}\n{}\n", at(200, "b"), at(300, "c")));
    }

    #[test]
    fn window_without_timestamps_gives_the_last_lines() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let lines: Vec<String> = (0..10).map(|n| format!("untimed {n}")).collect();
        assert!(lines.iter().all(|line| line.len() + 1 == 10));
        let file = log_file(dir.path(), "beep.log", &lines);
        let text = window(&[file], 150, 35).expect("the file reads");
        assert_eq!(text, "untimed 7\nuntimed 8\nuntimed 9\n");
    }

    #[test]
    fn window_keeps_the_newest_whole_lines_under_the_cap() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let lines: Vec<String> = (0..5).map(|n| at(100 + n, "line")).collect();
        let file = log_file(dir.path(), "beep.log", &lines);
        let line = lines[0].len() + 1;
        let text = window(&[file], 0, 2 * line + line / 2).expect("the file reads");
        assert_eq!(text, format!("{}\n{}\n", lines[3], lines[4]));
    }
}
