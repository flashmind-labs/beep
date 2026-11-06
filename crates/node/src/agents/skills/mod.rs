//! The user's own coding agent skills, copied to every machine of the mesh.
//! Each node lists its skills on `skills/1` (`disk`), reads every other
//! node's list, and takes a skill it never held when the machines that hold
//! it hold it alike (`compare`). A name with two contents is a conflict: it
//! is logged and told to the desktop, and no file changes. Deletes never
//! spread, and Beep's own installed skills stay out of it.

mod compare;
mod disk;
mod follow;

pub(crate) use follow::{follow, serve};

use beep_proto::node::skills::Conflict;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::watch;

/// Whether the node syncs skills, and the conflicts it last found. Shared by
/// every copy of the offers.
#[derive(Clone)]
pub struct Skills {
    /// The home folder holding the agents' skills folders; none when the
    /// host did not ask for skills to sync.
    home: Option<PathBuf>,
    conflicts: Arc<watch::Sender<Vec<Conflict>>>,
}

impl Default for Skills {
    fn default() -> Self {
        Self {
            home: None,
            conflicts: Arc::new(watch::Sender::new(Vec::new())),
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
