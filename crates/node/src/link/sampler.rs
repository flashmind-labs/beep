//! The machine's free disk, memory, load and traffic, sampled once for every
//! stream that follows them. Neither has a push of its own: the OS reports
//! disks and the endpoint reports bytes only when asked. So one task asks,
//! while a stream follows, and the streams send what it publishes.

use crate::lock;
use beep_proto::node::{StatsReport, traffic::TrafficReport};
use futures::future::BoxFuture;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    select,
    sync::watch,
    task::JoinSet,
    time::{MissedTickBehavior, interval},
};
use tokio_util::task::AbortOnDropHandle;

/// How often free disk, memory and the traffic counters are sampled.
const RESAMPLE: Duration = Duration::from_secs(60);

/// What was last sampled. A part is `None` until its first sample.
#[derive(Clone, Default)]
pub(crate) struct Sample {
    pub(crate) stats: Option<StatsReport>,
    pub(crate) traffic: Option<TrafficReport>,
}

/// The node's sampler. Clones share it.
#[derive(Clone, Default)]
pub(crate) struct Sampler(Arc<Shared>);

#[derive(Default)]
struct Shared {
    sample: watch::Sender<Sample>,
    /// The sampling task: running while a stream follows, none otherwise.
    task: Mutex<Option<AbortOnDropHandle<()>>>,
}

/// Measures the machine's disks, memory and load.
pub(crate) type Measure = Box<dyn Fn() -> BoxFuture<'static, StatsReport> + Send + Sync>;
/// Reads the traffic counters.
pub(crate) type Counted = Box<dyn Fn() -> TrafficReport + Send + Sync>;

impl Sampler {
    /// What the sampler publishes, starting it when it is not running. The
    /// receiver holds the last sample as unseen, so a new stream sends it at
    /// once. Sampling stops when the last receiver is dropped.
    pub(crate) fn follow(&self, measure: Measure, counted: Counted) -> watch::Receiver<Sample> {
        let mut task = lock(&self.0.task);
        let mut receiver = self.0.sample.subscribe();
        receiver.mark_changed();

        if task.is_none() {
            let sampling = sampling(self.0.clone(), measure, counted);
            *task = Some(AbortOnDropHandle::new(tokio::spawn(sampling)));
        }

        receiver
    }
}

/// Samples every `RESAMPLE` until nobody follows. The disks are read in a
/// task of their own, one at a time: a mount that hangs holds up the disks'
/// numbers and nothing else.
async fn sampling(shared: Arc<Shared>, measure: Measure, counted: Counted) {
    let mut ticks = interval(RESAMPLE);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut disks = JoinSet::new();

    loop {
        select! {
            _ = ticks.tick() => {
                let counted = counted();
                shared.sample.send_if_modified(|sample| {
                    let changed = sample.traffic.as_ref() != Some(&counted);
                    sample.traffic = Some(counted);
                    changed
                });

                if disks.is_empty() {
                    disks.spawn(measure());
                }
            }
            Some(done) = disks.join_next() => {
                if let Ok(measured) = done {
                    shared.sample.send_if_modified(|sample| {
                        let changed = sample.stats.as_ref() != Some(&measured);
                        sample.stats = Some(measured);
                        changed
                    });
                }
            }
            () = shared.sample.closed() => {
                // A stream may follow between the close and this lock.
                let mut task = lock(&shared.task);

                if shared.sample.receiver_count() == 0 {
                    *task = None;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/link_sampler.rs"]
mod tests;
