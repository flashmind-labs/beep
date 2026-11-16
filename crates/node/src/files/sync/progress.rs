//! How far one round of bringing files in line is, and how fast it moves.
//! Times are passed in, so it runs on any clock.

use beep_proto::node::sync::{FileProgress, MAX_WAITING, Progress};
use std::collections::VecDeque;
use tokio::time::{Duration, Instant};

/// How far back the rate looks.
const WINDOW: Duration = Duration::from_secs(10);
/// The shortest span a rate is told over.
const MIN_SPAN: Duration = Duration::from_millis(500);

/// One point of the round: when, and the bytes brought by then.
#[derive(Debug, Clone, Copy)]
struct Sample {
    at: Instant,
    bytes: u64,
}

/// One round's counts, and the recent points the rate comes from. A file
/// counts as done once it is in line or failed, its bytes with it. Bytes
/// that arrive from peers count as they come, so one large file moves the
/// bytes before it is done.
#[derive(Debug)]
pub(super) struct Meter {
    files_done: u64,
    files_total: u64,
    bytes_done: u64,
    bytes_total: u64,
    /// The bytes of the files brought in line, which the rate is told from.
    brought: u64,
    /// The bytes fetched from peers this round, done or not.
    received: u64,
    /// The file the round waits on, while it comes.
    file: Option<FileProgress>,
    samples: VecDeque<Sample>,
}

impl Meter {
    /// A round of `files_total` files and `bytes_total` bytes, starting at
    /// `now`.
    pub(super) fn new(files_total: u64, bytes_total: u64, now: Instant) -> Self {
        Self {
            files_done: 0,
            files_total,
            bytes_done: 0,
            bytes_total,
            brought: 0,
            received: 0,
            file: None,
            samples: VecDeque::from([Sample { at: now, bytes: 0 }]),
        }
    }

    /// A file of `bytes` was brought in line at `now`.
    pub(super) fn done(&mut self, bytes: u64, now: Instant) {
        self.files_done = self.files_done.saturating_add(1);
        self.bytes_done = self.bytes_done.saturating_add(bytes);
        self.brought = self.brought.saturating_add(bytes);
        self.sample(now);
    }

    /// `bytes` came from peers this round so far, at `now`.
    pub(super) fn received(&mut self, bytes: u64, now: Instant) {
        if bytes <= self.received {
            return;
        }

        self.received = bytes;
        self.sample(now);
    }

    /// The file the round waits on while it comes, or `None` once it is here.
    pub(super) fn waits_on(&mut self, file: Option<FileProgress>) {
        self.file = file;
    }

    /// The bytes that moved: those brought in line, or those received when
    /// the fetches are ahead of the writes.
    fn moved(&self) -> u64 {
        self.brought.max(self.received.min(self.bytes_total))
    }

    fn sample(&mut self, now: Instant) {
        self.samples.push_back(Sample {
            at: now,
            bytes: self.moved(),
        });
        // Keeps the last point before the window, so a slow file still
        // spans its whole time.
        while self
            .samples
            .get(1)
            .is_some_and(|next| now.saturating_duration_since(next.at) > WINDOW)
        {
            self.samples.pop_front();
        }
    }

    /// A file of `bytes` failed: it is done for this round, and its bytes
    /// with it, but it moves no rate.
    pub(super) fn failed(&mut self, bytes: u64) {
        self.files_done = self.files_done.saturating_add(1);
        self.bytes_done = self.bytes_done.saturating_add(bytes);
    }

    /// Bytes a second over the window; 0 before it can tell.
    pub(super) fn rate(&self) -> u64 {
        let (Some(first), Some(last)) = (self.samples.front(), self.samples.back()) else {
            return 0;
        };
        let span = last.at.saturating_duration_since(first.at);

        if span < MIN_SPAN {
            return 0;
        }

        let moved = u128::from(last.bytes.saturating_sub(first.bytes));
        u64::try_from(moved * 1000 / span.as_millis()).unwrap_or(u64::MAX)
    }

    pub(super) fn progress(&self) -> Progress {
        let rate = self.rate();
        let bytes_done = self.bytes_done.max(self.moved());
        Progress {
            files_done: self.files_done,
            files_total: self.files_total,
            bytes_done,
            bytes_total: self.bytes_total,
            rate,
            eta_secs: eta(self.bytes_total.saturating_sub(bytes_done), rate),
            file: self.file.clone(),
        }
    }
}

/// Seconds to move `left` bytes at `rate` bytes a second; `None` when the
/// rate is not known yet, or it would take longer than a `u32` holds.
pub(super) fn eta(left: u64, rate: u64) -> Option<u32> {
    if left == 0 {
        return Some(0);
    }

    if rate == 0 {
        return None;
    }

    u32::try_from(left.div_ceil(rate)).ok()
}

/// What still waits: the paths that failed, then those not reached yet,
/// at most `MAX_WAITING`.
pub(super) fn waiting(failed: &[String], ahead: &[String]) -> Vec<String> {
    failed
        .iter()
        .chain(ahead)
        .take(MAX_WAITING)
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "../../tests/files_sync_progress.rs"]
mod tests;
