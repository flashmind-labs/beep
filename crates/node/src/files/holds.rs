//! The checkouts waiting for Git recovery to be let run.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;

/// The checkouts whose local edits wait for Git recovery, published by the
/// sync that wants it. Whoever may let it run follows this, checks the
/// checkout is idle and answers each root through `Syncs::answer_recovery`.
#[derive(Clone, Default)]
pub struct Recoveries(Arc<watch::Sender<BTreeSet<PathBuf>>>);

impl Recoveries {
    /// The roots waiting for an answer, changing on every ask.
    pub fn subscribe(&self) -> watch::Receiver<BTreeSet<PathBuf>> {
        self.0.subscribe()
    }

    pub(crate) fn ask(&self, root: &Path) {
        self.0.send_modify(|roots| {
            roots.insert(root.to_owned());
        });
    }

    pub(crate) fn answered(&self, root: &Path) {
        self.0.send_if_modified(|roots| roots.remove(root));
    }
}
