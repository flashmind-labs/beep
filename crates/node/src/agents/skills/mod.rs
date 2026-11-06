//! The user's own coding agent skills, copied to every machine of the mesh.
//! Each node lists its skills on `skills/1` (`disk`), reads every other
//! node's list, and takes a skill it never held, or a newer edit of one it
//! synced (`compare`). Copies edited apart are a conflict: it is logged and
//! told to the desktop, and no file changes until the user keeps one copy
//! (`Request::Keep`, sent to the machine holding it). Deletes never spread,
//! and Beep's own installed skills stay out of it.

mod compare;
mod disk;
mod follow;

pub(crate) use follow::{follow, serve};

use beep_proto::node::skills::{Conflict, Listed};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::{Notify, watch};

/// Whether the node syncs skills, what it lists and the conflicts it last
/// found. Shared by every copy of the offers; the `follow` task writes it.
#[derive(Clone)]
pub struct Skills {
    /// The home folder holding the agents' skills folders; none when the
    /// host did not ask for skills to sync.
    home: Option<PathBuf>,
    /// This machine's copies as the last pass listed them; none before the
    /// first.
    listed: Arc<watch::Sender<Option<Vec<Listed>>>>,
    conflicts: Arc<watch::Sender<Vec<Conflict>>>,
    /// The copies the user kept here, by name to hash, for the next pass.
    picks: Arc<watch::Sender<BTreeMap<String, String>>>,
    /// Rung when another machine says its skills changed.
    nudge: Arc<Notify>,
}

impl Default for Skills {
    fn default() -> Self {
        Self {
            home: None,
            listed: Arc::new(watch::Sender::new(None)),
            conflicts: Arc::new(watch::Sender::new(Vec::new())),
            picks: Arc::new(watch::Sender::new(BTreeMap::new())),
            nudge: Arc::new(Notify::new()),
        }
    }
}

impl Skills {
    /// Syncs the skills under `home`.
    pub fn under(home: PathBuf) -> Self {
        Self {
            home: Some(home),
            ..Self::default()
        }
    }

    pub(crate) fn home(&self) -> Option<&PathBuf> {
        self.home.as_ref()
    }
}
