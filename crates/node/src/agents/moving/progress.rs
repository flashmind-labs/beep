//! What the moves this node takes part in have done so far, for the
//! target to answer `MoveProgress`.

use crate::lock;
use beep_proto::node::agent::{MovePhase, MoveProgress};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

/// One move's progress, written by its task.
pub(super) type Tracker = Arc<watch::Sender<MoveProgress>>;

#[derive(Default)]
pub(crate) struct Moves {
    tracked: Mutex<HashMap<String, Tracker>>,
}

impl Moves {
    /// A fresh tracker for the move of session `id`.
    pub(super) fn begin(&self, id: &str) -> Tracker {
        let tracker = Arc::new(watch::Sender::new(MoveProgress::default()));
        lock(&self.tracked).insert(id.to_owned(), tracker.clone());
        tracker
    }

    /// How the move of `id` stands here, when this machine is part of one.
    pub(crate) fn progress(&self, id: &str) -> Option<MoveProgress> {
        lock(&self.tracked)
            .get(id)
            .map(|tracker| tracker.borrow().clone())
    }

    /// Whether the move of `id` is past the point where it can be
    /// cancelled: the session is starting or running here.
    pub(super) fn committed(&self, id: &str) -> bool {
        self.progress(id)
            .is_some_and(|progress| matches!(progress.phase, MovePhase::Resuming | MovePhase::Done))
    }
}

/// Changes the progress shown, keeping what `change` leaves alone.
pub(super) fn show(tracker: &Tracker, change: impl FnOnce(&mut MoveProgress)) {
    tracker.send_modify(change);
}
