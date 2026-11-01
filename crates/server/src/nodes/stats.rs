//! The live resources each node reports with `Up::Stats`, kept in memory
//! for as long as its connection lasts; there is no history.

use super::Online;
use crate::protocol::node::StatsReport;
use std::{
    sync::{Mutex, PoisonError},
    time::SystemTime,
};

/// A node's latest stats and when they came.
#[derive(Debug, Clone, PartialEq)]
pub struct Sampled {
    pub at: SystemTime,
    pub report: StatsReport,
}

/// Where a connection keeps its node's latest stats.
#[derive(Debug, Default)]
pub(super) struct StatsSlot(Mutex<Option<Sampled>>);

impl StatsSlot {
    pub(super) fn set(&self, report: StatsReport) {
        let sampled = Sampled {
            at: SystemTime::now(),
            report,
        };
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(sampled);
    }

    fn get(&self) -> Option<Sampled> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Online {
    /// What the node last said it has free on this connection; `None`
    /// until it says, and always from a node from before `Up::Stats`.
    pub fn stats(&self) -> Option<Sampled> {
        self.stats.get()
    }
}
