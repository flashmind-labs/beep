//! Project locations, and the removed ones whose sessions stay out of the menu.

use beep_proto::{ImportedProject, MachineSession};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Place {
    pub project: String,
    pub machine: String,
    pub folder: String,
}

impl From<&ImportedProject> for Place {
    fn from(project: &ImportedProject) -> Self {
        Self {
            project: project.project.clone(),
            machine: project.machine_id.clone(),
            folder: project.folder.clone(),
        }
    }
}

impl Place {
    pub fn contains(&self, session: &MachineSession) -> bool {
        self.machine == session.machine_id && within(&session.cwd, &self.folder)
    }
}

/// A removed nested project still wins over its surviving parent. An
/// explicit import at the same location makes that location searchable again.
pub(crate) fn retired(
    session: &MachineSession,
    active: &[ImportedProject],
    removed: &[Place],
) -> bool {
    let current = active
        .iter()
        .filter(|p| p.machine_id == session.machine_id && within(&session.cwd, &p.folder))
        .map(|p| p.folder.len())
        .max();
    let old = removed
        .iter()
        .filter(|p| p.contains(session))
        .map(|p| p.folder.len())
        .max();
    old.is_some_and(|old| current.is_none_or(|current| old > current))
}

/// True when `cwd` is `folder` or inside it.
pub(crate) fn within(cwd: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches(['/', '\\']);
    !folder.is_empty()
        && cwd
            .strip_prefix(folder)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
}
