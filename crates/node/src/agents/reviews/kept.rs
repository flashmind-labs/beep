//! The reviews this node ran and runs, and the one that lists them.

use crate::agents::family::Who;
use beep_proto::SessionId;
use beep_proto::node::agent::ReviewRun;
use std::{collections::VecDeque, sync::Mutex};
use tokio::sync::Notify;
use tokio_util::task::AbortOnDropHandle;

/// The most reviews kept, finished ones going first.
const MAX_REVIEWS: usize = 64;
/// The reviews this node ran and runs.
pub(crate) struct Reviews {
    pub(super) kept: Mutex<VecDeque<Kept>>,
    pub(super) events: crate::events::Hub,
    /// Woken when a review changes, for the callers that wait on one.
    pub(super) changed: Notify,
}

pub(super) struct Kept {
    pub(super) run: ReviewRun,
    /// Who asked, as the family names the parent.
    pub(super) parent: Option<Who>,
    /// The parent's id on this node.
    pub(super) parent_key: Option<SessionId>,
    /// Follows the reviewer until it is done.
    pub(super) _watch: Option<AbortOnDropHandle<()>>,
}

impl Default for Reviews {
    fn default() -> Self {
        Self::new(crate::events::system().clone())
    }
}

impl Reviews {
    pub(crate) fn new(events: crate::events::Hub) -> Self {
        Self {
            kept: Mutex::default(),
            events,
            changed: Notify::new(),
        }
    }

    pub(super) fn with<T>(&self, id: &SessionId, f: impl FnOnce(&mut Kept) -> T) -> Option<T> {
        let mut all = crate::lock(&self.kept);
        let kept = all.iter_mut().find(|kept| kept.run.id == *id)?;
        let before = kept.run.clone();
        let value = f(kept);

        if kept.run != before {
            self.events
                .changed(beep_proto::node::events::Topic::keyed("reviews", id));
            self.changed.notify_waiters();
        }

        Some(value)
    }

    pub(super) fn keep(&self, kept: Kept) {
        let mut all = crate::lock(&self.kept);

        if all.len() >= MAX_REVIEWS
            && let Some(at) = all.iter().position(|kept| !kept.run.running())
        {
            all.remove(at);
        }

        self.events.changed(beep_proto::node::events::Topic::keyed(
            "reviews",
            &kept.run.id,
        ));
        all.push_back(kept);
    }
}
