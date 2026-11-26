//! What the log readers share: the options, and the printer that filters
//! lines by time and noise and keeps the last few.

use std::{
    borrow::Cow,
    collections::VecDeque,
    io::{self, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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

/// The earliest second shown, in Unix time.
pub(super) fn cutoff(since: Duration) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |now| now.as_secs());
    let since = since.as_secs();
    i64::try_from(now.saturating_sub(since)).unwrap_or(i64::MAX)
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
pub(super) struct Head<'a> {
    /// Unix seconds.
    pub(super) time: i64,
    pub(super) level: &'a str,
    pub(super) target: &'a str,
}

/// The head of `line`, which has had its colors removed; `None` for a line
/// that does not start with a timestamp, such as a panic or a message from
/// the service manager.
pub(super) fn head(line: &str) -> Option<Head<'_>> {
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
pub(super) fn is_noise(head: &Head<'_>) -> bool {
    matches!(head.level, "INFO" | "DEBUG" | "TRACE") && noisy_target(head.target)
}

/// `line` without ANSI color codes, which the node writes when its stderr
/// is a file (macOS).
pub(super) fn plain(line: &str) -> Cow<'_, str> {
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
pub(super) fn parse_time(text: &str) -> Option<i64> {
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
pub(super) struct Filter {
    /// Shows the noise too.
    pub(super) all: bool,
    /// Lines before this Unix second are left out; `None` keeps them all.
    pub(super) cutoff: Option<i64>,
    /// Whether the last timestamped line was at or after the cutoff. Lines
    /// without a timestamp go with it.
    pub(super) in_window: bool,
    /// Whether a line with a timestamp came.
    pub(super) timed: bool,
}

impl Filter {
    pub(super) fn new(all: bool, cutoff: Option<i64>) -> Self {
        Self {
            all,
            cutoff,
            in_window: cutoff.is_none(),
            timed: false,
        }
    }

    /// Whether to show `line`.
    pub(super) fn keep(&mut self, line: &str) -> bool {
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
pub(super) struct Tail {
    pub(super) cap: usize,
    /// The most bytes kept, newlines included.
    pub(super) max_bytes: usize,
    pub(super) bytes: usize,
    pub(super) lines: VecDeque<String>,
}

impl Tail {
    pub(super) fn new(cap: usize) -> Self {
        Self {
            cap,
            max_bytes: usize::MAX,
            bytes: 0,
            lines: VecDeque::with_capacity(cap.min(4096)),
        }
    }

    /// As many of the last lines as fit in `max_bytes`.
    pub(super) fn bytes(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            ..Self::new(usize::MAX)
        }
    }

    pub(super) fn push(&mut self, line: &str) {
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
pub(super) struct Printer<W: Write> {
    pub(super) filter: Filter,
    pub(super) tail: Option<Tail>,
    pub(super) out: W,
}

impl<W: Write> Printer<W> {
    pub(super) fn new(options: &LogOptions, cutoff: i64, out: W) -> Self {
        Self {
            filter: Filter::new(options.all, Some(cutoff)),
            tail: options.lines.map(Tail::new),
            out,
        }
    }

    /// Takes one line, without its newline.
    pub(super) fn line(&mut self, line: &str) -> io::Result<()> {
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
    pub(super) fn bytes(&mut self, line: &[u8]) -> io::Result<()> {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        self.line(&String::from_utf8_lossy(line))
    }

    /// Ends the past: prints the tail kept, and from here on every line as
    /// it comes.
    pub(super) fn caught_up(&mut self) -> io::Result<()> {
        if let Some(tail) = self.tail.take() {
            for line in tail.lines {
                writeln!(self.out, "{line}")?;
            }
        }

        self.out.flush()
    }
}
