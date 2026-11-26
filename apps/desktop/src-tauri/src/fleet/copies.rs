//! What is known about the folders asked about so far, kept by `Fleet` for `work_copy`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long an answer about one folder stands. Reads come a part at a time,
/// and a pause or a new sync shows within this.
const FRESH: Duration = Duration::from_secs(60);
/// Folders whose answers are kept at once.
const MAX_KEPT: usize = 256;

/// What is known about the folders asked about so far.
#[derive(Default)]
pub(crate) struct Copies {
    known: Mutex<HashMap<Asked, Known>>,
}

#[derive(PartialEq, Eq, Hash)]
pub(super) struct Asked {
    pub(super) machine_id: String,
    pub(super) folder: String,
}

struct Known {
    at: Instant,
    copy: Option<LocalCopy>,
}

/// A project's folder on the machine, and where sync keeps it on this Mac.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LocalCopy {
    pub(super) remote: String,
    pub(super) local: PathBuf,
}

impl Copies {
    pub(super) fn kept(&self, asked: &Asked) -> Option<Option<LocalCopy>> {
        let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let found = known.get(asked)?;
        (found.at.elapsed() < FRESH).then(|| found.copy.clone())
    }

    /// Whether no answer about the folder was standing: reads running at
    /// once all look, and their miss is logged once.
    pub(super) fn keep(&self, asked: Asked, copy: Option<LocalCopy>) -> bool {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);

        if known.len() >= MAX_KEPT {
            known.clear();
        }

        let first = known
            .get(&asked)
            .is_none_or(|found| found.at.elapsed() >= FRESH);
        let at = Instant::now();
        known.insert(asked, Known { at, copy });
        first
    }
}

#[cfg(test)]
#[path = "../tests/fleet_copies.rs"]
mod tests;
