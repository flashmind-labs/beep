//! How far one round of bringing files in line is, and how fast it moves.
//! Times are passed in, so it runs on any clock.

use beep_proto::node::sync::{MAX_WAITING, Progress};
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
/// counts as done once it is in line or failed, its bytes with it, so the
/// files and the bytes done tell the same part of the round.
#[derive(Debug)]
pub(super) struct Meter {
    files_done: u64,
    files_total: u64,
    bytes_done: u64,
    bytes_total: u64,
    /// The bytes of the files brought in line, which the rate is told from.
    brought: u64,
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
            samples: VecDeque::from([Sample { at: now, bytes: 0 }]),
        }
    }

    /// A file of `bytes` was brought in line at `now`.
    pub(super) fn done(&mut self, bytes: u64, now: Instant) {
        self.files_done = self.files_done.saturating_add(1);
        self.bytes_done = self.bytes_done.saturating_add(bytes);
        self.brought = self.brought.saturating_add(bytes);
        self.samples.push_back(Sample {
            at: now,
            bytes: self.brought,
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
        Progress {
            files_done: self.files_done,
            files_total: self.files_total,
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total,
            rate,
            eta_secs: eta(self.bytes_total.saturating_sub(self.bytes_done), rate),
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
mod tests {
    use super::*;

    #[test]
    fn the_rate_follows_the_last_ten_seconds() {
        let start = Instant::now();
        let mut meter = Meter::new(100, 100_000_000, start);
        // Fast at first, 1 MB a second for 20 seconds...
        for s in 1..=20 {
            meter.done(1_000_000, start + Duration::from_secs(s));
        }
        // ...then 100 KB a second for 20 more.
        for s in 21..=40 {
            meter.done(100_000, start + Duration::from_secs(s));
        }
        let rate = meter.rate();
        assert!((100_000..=200_000).contains(&rate), "rate {rate}");
    }

    #[test]
    fn one_slow_file_spans_its_whole_time() {
        let start = Instant::now();
        let mut meter = Meter::new(1, 60_000_000, start);
        meter.done(60_000_000, start + Duration::from_secs(60));
        assert_eq!(meter.rate(), 1_000_000);
    }

    #[test]
    fn failed_files_count_their_bytes_as_done() {
        // Files that were already here and failed to go in place: the bytes
        // done follow the files done, not stay at nothing.
        let start = Instant::now();
        let mut meter = Meter::new(4, 400, start);
        meter.failed(100);
        meter.failed(100);
        meter.done(100, start + Duration::from_secs(1));
        let p = meter.progress();
        assert_eq!((p.files_done, p.files_total), (3, 4));
        assert_eq!((p.bytes_done, p.bytes_total), (300, 400));
        assert_eq!(p.rate, 100);
        assert_eq!(p.eta_secs, Some(1));
    }

    #[test]
    fn failed_paths_wait_first_and_the_list_is_bounded() {
        let ahead: Vec<String> = (0..100).map(|i| format!("f{i}")).collect();
        let out = waiting(&["bad".into()], &ahead);
        assert_eq!(out.len(), MAX_WAITING);
        assert_eq!(out[0], "bad");
        assert_eq!(out[1], "f0");
    }
}
