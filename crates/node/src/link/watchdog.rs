//! Says in the log when the node stops making progress. A task on the async
//! workers beats every few seconds, and the steps that have hung before mark
//! when they start and end. A plain thread, which needs no async worker,
//! checks both and logs a warning naming what has waited too long.
//!
//! Only atomics: the watchdog must not wait on anything the node could hold.

use std::{
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tokio::time::interval;
use tokio_util::task::AbortOnDropHandle;

/// How long the workers or a step may go before it is logged.
const STALL: Duration = Duration::from_secs(30);
/// How often the workers beat and the thread looks.
const BEAT: Duration = Duration::from_secs(5);
/// How often a stall that goes on is logged again.
const AGAIN: Duration = Duration::from_secs(300);

/// A step the node has hung in before.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// A `beep-node mesh` command on the control socket.
    MeshCommand,
    /// Writing down what is open, for the next start.
    OpenNote,
    /// Applying the projects the mesh syncs.
    SyncApply,
}

impl Step {
    const ALL: [Step; 3] = [Step::MeshCommand, Step::OpenNote, Step::SyncApply];

    fn as_str(self) -> &'static str {
        match self {
            Step::MeshCommand => "a mesh command on the control socket",
            Step::OpenNote => "the note of open sessions",
            Step::SyncApply => "applying the mesh's sync set",
        }
    }
}

/// How many run, and since when the first of them, in ms from `START`.
struct Busy {
    running: AtomicUsize,
    since_ms: AtomicU64,
}

impl Busy {
    const fn new() -> Self {
        Self {
            running: AtomicUsize::new(0),
            since_ms: AtomicU64::new(0),
        }
    }
}

/// One per `Step`, in its order.
static BUSY: [Busy; Step::ALL.len()] = [Busy::new(), Busy::new(), Busy::new()];
/// When the workers last beat, in ms from `START`.
static BEATEN_MS: AtomicU64 = AtomicU64::new(0);
static START: LazyLock<Instant> = LazyLock::new(Instant::now);

fn now_ms() -> u64 {
    u64::try_from(START.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn busy(step: Step) -> &'static Busy {
    &BUSY[step as usize]
}

/// Marks `step` running until dropped.
pub(crate) struct Running(Step);

/// Marks `step` as running until the result is dropped.
pub(crate) fn enter(step: Step) -> Running {
    let busy = busy(step);
    // The first in sets the time; one racing it changes a log line at most.
    if busy.running.fetch_add(1, Ordering::Relaxed) == 0 {
        busy.since_ms.store(now_ms(), Ordering::Relaxed);
    }
    Running(step)
}

impl Drop for Running {
    fn drop(&mut self) {
        busy(self.0).running.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The watchdog: the beat on the async workers and the thread that looks.
/// Both stop when it is dropped.
pub(crate) struct Watchdog {
    stop: Arc<AtomicBool>,
    _beat: AbortOnDropHandle<()>,
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Starts the watchdog; call it on the node's runtime.
pub(crate) fn start() -> Watchdog {
    BEATEN_MS.store(now_ms(), Ordering::Relaxed);
    let beat = AbortOnDropHandle::new(tokio::spawn(async {
        let mut beat = interval(BEAT);
        loop {
            beat.tick().await;
            BEATEN_MS.store(now_ms(), Ordering::Relaxed);
        }
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let looking = thread::Builder::new()
        .name("watchdog".to_owned())
        .spawn(move || look(&stopped));
    if let Err(e) = looking {
        log::warn!("no watchdog: {e}");
    }
    Watchdog { stop, _beat: beat }
}

/// What has waited longer than `STALL` at `now`, as log text, if anything.
fn stalls(now: u64) -> Option<String> {
    let stall = u64::try_from(STALL.as_millis()).unwrap_or(u64::MAX);
    let mut what = Vec::new();
    let quiet = now.saturating_sub(BEATEN_MS.load(Ordering::Relaxed));
    if quiet >= stall {
        what.push(format!(
            "the async workers have not run for {}s",
            quiet / 1000
        ));
    }
    for step in Step::ALL {
        let busy = busy(step);
        if busy.running.load(Ordering::Relaxed) == 0 {
            continue;
        }
        let waited = now.saturating_sub(busy.since_ms.load(Ordering::Relaxed));
        if waited >= stall {
            what.push(format!("{} has run for {}s", step.as_str(), waited / 1000));
        }
    }
    (!what.is_empty()).then(|| what.join("; "))
}

/// Looks every `BEAT` until `stop`, and logs a stall once, then every
/// `AGAIN` while it lasts.
fn look(stop: &AtomicBool) {
    let mut told: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(BEAT);
        match stalls(now_ms()) {
            Some(what) if told.is_none_or(|at| at.elapsed() >= AGAIN) => {
                log::warn!("the node is stuck: {what}");
                told = Some(Instant::now());
            }
            Some(_) => {}
            None => {
                if told.take().is_some() {
                    log::warn!("the node is moving again");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_that_runs_too_long_is_named() {
        let _running = enter(Step::SyncApply);
        let later = busy(Step::SyncApply).since_ms.load(Ordering::Relaxed)
            + u64::try_from(STALL.as_millis()).expect("small");
        let what = stalls(later).expect("a stall");
        assert!(
            what.contains("applying the mesh's sync set has run for"),
            "{what}"
        );
    }
}
