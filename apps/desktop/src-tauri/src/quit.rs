//! Coordinates bounded async shutdown so the UI event loop keeps servicing
//! WebKit while Tokio tasks stop, and `RunEvent::Exit` never calls `block_on`.

use crate::store::lock;
use std::{
    fmt::Display,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(desktop)]
use std::thread;

#[cfg(target_os = "macos")]
use std::sync::OnceLock;

#[cfg(desktop)]
type Waiter = Box<dyn FnOnce() + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StopOutcome {
    Done,
    Failed,
    TimedOut,
}

/// Signals completion of the async shutdown task to OS threads without polling Tokio.
pub(crate) struct Stopped {
    done: Mutex<Option<Result<(), String>>>,
    ready: Condvar,
    logged: AtomicBool,
}

impl Stopped {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            done: Mutex::new(None),
            ready: Condvar::new(),
            logged: AtomicBool::new(false),
        })
    }

    pub(crate) fn finish(&self, result: Result<(), impl Display>) {
        let mut done = lock(&self.done);
        if done.is_none() {
            *done = Some(result.map_err(|e| e.to_string()));
            self.ready.notify_all();
        }
    }

    pub(crate) fn wait(&self, deadline: Instant) -> StopOutcome {
        let mut done = lock(&self.done);

        while done.is_none() {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                self.log_timeout();
                return StopOutcome::TimedOut;
            };
            let (next, _) = self
                .ready
                .wait_timeout(done, remaining)
                .unwrap_or_else(PoisonError::into_inner);
            done = next;
        }

        match done.as_ref() {
            Some(Ok(())) => StopOutcome::Done,
            Some(Err(error)) => {
                self.log_error(error);
                StopOutcome::Failed
            }
            None => StopOutcome::TimedOut,
        }
    }

    fn log_error(&self, error: &str) {
        if !self.logged.swap(true, Ordering::AcqRel) {
            log::warn!("stopping the mesh failed: {error}");
        }
    }

    fn log_timeout(&self) {
        if !self.logged.swap(true, Ordering::AcqRel) {
            log::warn!("quitting before the mesh stopped");
        }
    }
}

/// Marks `Stopped` failed if the shutdown task is aborted or panics before finishing.
pub(crate) struct StopGuard {
    stopped: Option<Arc<Stopped>>,
}

impl StopGuard {
    pub(crate) fn new(stopped: Arc<Stopped>) -> Self {
        Self {
            stopped: Some(stopped),
        }
    }

    pub(crate) fn succeed(&mut self) {
        if let Some(stopped) = self.stopped.take() {
            stopped.finish(Ok::<(), &str>(()));
        }
    }
}

impl Drop for StopGuard {
    fn drop(&mut self) {
        if let Some(stopped) = self.stopped.take() {
            stopped.finish(Err("shutdown task stopped early"));
        }
    }
}

#[derive(Default)]
enum Phase {
    #[default]
    Running,
    #[cfg(desktop)]
    Stopping {
        stopped: Arc<Stopped>,
        deadline: Instant,
        waiters: Vec<Waiter>,
    },
    ReadyToExit,
}

#[derive(Default)]
pub(crate) struct Quitting {
    phase: Mutex<Phase>,
    finished: AtomicBool,
}

impl Quitting {
    /// Starts `start` once and runs `on_ready` from an OS waiter thread once
    /// `Stopped` completes or `limit` elapses. Returns `true` while the caller
    /// must defer exiting so the UI event loop keeps running, or `false` once
    /// stopping has finished and the process may exit now.
    #[cfg(desktop)]
    pub(crate) fn defer_or_exit_now(
        self: &Arc<Self>,
        limit: Duration,
        start: impl FnOnce(Arc<Stopped>),
        on_ready: impl FnOnce() + Send + 'static,
    ) -> bool {
        let (stopped, deadline) = {
            let mut phase = lock(&self.phase);

            match &mut *phase {
                Phase::ReadyToExit => return false,
                Phase::Stopping { waiters, .. } => {
                    waiters.push(Box::new(on_ready));
                    return true;
                }
                Phase::Running => {
                    let stopped = Stopped::new();
                    let deadline = Instant::now() + limit;
                    *phase = Phase::Stopping {
                        stopped: stopped.clone(),
                        deadline,
                        waiters: vec![Box::new(on_ready)],
                    };
                    (stopped, deadline)
                }
            }
        };

        start(Arc::clone(&stopped));
        let quitting = Arc::clone(self);
        let waiting = Arc::clone(&stopped);
        let spawned = thread::Builder::new()
            .name("beep-quit".into())
            .spawn(move || {
                waiting.wait(deadline);
                for waiter in quitting.take_waiters() {
                    waiter();
                }
            });

        if let Err(error) = spawned {
            log::warn!("spawning the quit waiter failed: {error}");
            stopped.wait(deadline);
            for waiter in self.take_waiters() {
                waiter();
            }
        }

        true
    }

    /// Runs `start` if nothing started it yet, waits up to `limit` on an OS
    /// condvar, and returns `true` once so final synchronous flushes run once.
    pub(crate) fn finish(&self, limit: Duration, start: impl FnOnce(Arc<Stopped>)) -> bool {
        if self.finished.swap(true, Ordering::AcqRel) {
            return false;
        }

        enum Pending {
            Start,
            #[cfg(desktop)]
            Wait(Arc<Stopped>, Instant),
            Done,
        }

        let pending = {
            let mut phase = lock(&self.phase);
            let prev = std::mem::replace(&mut *phase, Phase::ReadyToExit);

            match prev {
                Phase::Running => Pending::Start,
                #[cfg(desktop)]
                Phase::Stopping {
                    stopped, deadline, ..
                } => Pending::Wait(stopped, deadline),
                Phase::ReadyToExit => Pending::Done,
            }
        };

        match pending {
            Pending::Start => {
                let stopped = Stopped::new();
                let deadline = Instant::now() + limit;
                start(stopped.clone());
                stopped.wait(deadline);
            }
            #[cfg(desktop)]
            Pending::Wait(stopped, deadline) => {
                stopped.wait(deadline);
            }
            Pending::Done => {}
        }

        true
    }

    #[cfg(desktop)]
    fn take_waiters(&self) -> Vec<Waiter> {
        let mut phase = lock(&self.phase);
        let prev = std::mem::replace(&mut *phase, Phase::ReadyToExit);

        match prev {
            Phase::Stopping { waiters, .. } => waiters,
            other => {
                *phase = other;
                Vec::new()
            }
        }
    }
}

#[cfg(target_os = "macos")]
static MACOS_SHOULD_TERMINATE: OnceLock<Box<dyn Fn() -> bool + Send + Sync>> = OnceLock::new();

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn beep_install_terminate_hook(callback: extern "C" fn() -> bool);
    fn beep_finish_terminate();
}

#[cfg(target_os = "macos")]
extern "C" fn should_terminate_trampoline() -> bool {
    MACOS_SHOULD_TERMINATE.get().is_none_or(|decide| decide())
}

/// Hooks `-[NSApplicationDelegate applicationShouldTerminate:]` on the main
/// thread so `Cmd+Q`, Sparkle updates and AppleEvent quits stop the mesh while
/// `CFRunLoop` stays responsive.
#[cfg(target_os = "macos")]
pub(crate) fn install_macos_hook(should_terminate_now: impl Fn() -> bool + Send + Sync + 'static) {
    let _ = MACOS_SHOULD_TERMINATE.set(Box::new(should_terminate_now));
    // SAFETY: called on the main thread during Tauri setup after EventLoop::new
    // installed NSApp's delegate, passing a `'static` C function pointer.
    unsafe { beep_install_terminate_hook(should_terminate_trampoline) };
}

#[cfg(target_os = "macos")]
pub(crate) fn finish_macos_terminate() {
    // SAFETY: schedules `[NSApp replyToApplicationShouldTerminate:YES]` onto
    // `CFRunLoopGetMain()` and wakes it; safe from any thread.
    unsafe { beep_finish_terminate() };
}

#[cfg(test)]
#[path = "tests/quit.rs"]
mod tests;
