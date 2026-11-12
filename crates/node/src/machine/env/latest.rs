//! The machine's latest probe, shared by every `status/1` stream and
//! `list_machines`. One task probes while anyone follows it: at once when
//! the last report is old, every [`REPROBE`] and on a sign-in, never two
//! probes at a time.

use beep_proto::node::EnvReport;
use futures::future::BoxFuture;
use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::{sync::watch, time::Instant};

/// How often the machine is probed again while followed.
pub(crate) const REPROBE: Duration = Duration::from_secs(10 * 60);

/// A probe's report and when it was made.
#[derive(Clone, Debug)]
pub(crate) struct Probed {
    pub(crate) report: EnvReport,
    pub(crate) at: Instant,
}

type Probe = Arc<dyn Fn() -> BoxFuture<'static, EnvReport> + Send + Sync>;

pub(crate) struct Probes {
    latest: watch::Sender<Option<Probed>>,
    /// Whether the probing task runs.
    running: AtomicBool,
    probe: Probe,
}

static MACHINE: LazyLock<Arc<Probes>> =
    LazyLock::new(|| Probes::new(|| Box::pin(super::probe())));

/// This machine's probes.
pub(crate) fn machine() -> &'static Arc<Probes> {
    &MACHINE
}

/// A report of this machine no older than [`REPROBE`], probing for it when
/// the last one is older.
pub(crate) async fn fresh() -> EnvReport {
    let mut latest = machine().follow();
    let probed = latest
        .wait_for(|probed| probed.as_ref().is_some_and(|p| p.at.elapsed() < REPROBE))
        .await;

    match probed {
        Ok(probed) => probed.as_ref().map(|p| p.report.clone()).unwrap_or_default(),
        // The sender lives in `Probes`, which outlives this call.
        Err(_) => EnvReport::default(),
    }
}

impl Probes {
    pub(crate) fn new(
        probe: impl Fn() -> BoxFuture<'static, EnvReport> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            latest: watch::channel(None).0,
            running: AtomicBool::new(false),
            probe: Arc::new(probe),
        })
    }

    /// Follows the latest report, starting the probing task if it stopped.
    pub(crate) fn follow(self: &Arc<Self>) -> watch::Receiver<Option<Probed>> {
        let latest = self.latest.subscribe();

        if !self.running.swap(true, Ordering::SeqCst) {
            tokio::spawn(Self::keep_probing(Unfinished(Some(self.clone()))));
        }

        latest
    }

    /// A task dropped with its runtime, even before it first ran, never
    /// reaches `stop`: `guard` then lets the next follower start a new one.
    async fn keep_probing(mut guard: Unfinished) {
        if let Some(probes) = guard.0.clone() {
            probes.probe_while_followed().await;
        }
        guard.0 = None;
    }

    async fn probe_while_followed(&self) {
        let mut signed_in = crate::machine::sign_in::signed_in();
        signed_in.mark_unchanged();

        loop {
            let due = self
                .latest
                .borrow()
                .as_ref()
                .map_or_else(Instant::now, |probed| probed.at + REPROBE);

            tokio::select! {
                _ = tokio::time::sleep_until(due) => {}
                Ok(()) = signed_in.changed() => {}
                () = self.latest.closed() => {
                    if self.stop() {
                        return;
                    }
                    continue;
                }
            }

            tokio::select! {
                report = (self.probe)() => {
                    self.latest.send_replace(Some(Probed { report, at: Instant::now() }));
                }
                () = self.latest.closed() => {
                    if self.stop() {
                        return;
                    }
                }
            }
        }
    }

    /// Stops the task when nobody follows the reports; false when someone
    /// started following while it stopped, and it carries on.
    fn stop(&self) -> bool {
        self.running.store(false, Ordering::SeqCst);

        self.latest.receiver_count() == 0 || self.running.swap(true, Ordering::SeqCst)
    }
}

/// Marks the probing task stopped when it is dropped before it ends.
struct Unfinished(Option<Arc<Probes>>);

impl Drop for Unfinished {
    fn drop(&mut self) {
        if let Some(probes) = self.0.take() {
            probes.running.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/machine_env_latest.rs"]
mod tests;
