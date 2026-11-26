//! The lists the palette's index is built from, which projects, agents and the palette write.

use super::places::{Place, retired};
use beep_proto::{ImportedProject, Machine, MachineSession};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

/// The most sessions kept that the user opened and the mirror does not hold.
pub(crate) const OPENED_MOST: usize = 50;

/// The lists the index is built from. A list stays `None` until it was read
/// once; after that the last answer stays when a read fails.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct PaletteLists {
    pub(crate) machines: Option<Vec<Machine>>,
    pub(crate) projects: Option<Vec<ImportedProject>>,
    #[serde(default)]
    pub(crate) retired_places: Vec<Place>,
    #[serde(default)]
    pub(crate) forgotten_projects: HashSet<String>,
    pub(crate) branches: Option<Vec<BranchRow>>,
    /// Sessions the user opened that the mirror did not hold, such as one only in a project's
    /// history, newest last.
    #[serde(default)]
    pub(crate) opened: Vec<MachineSession>,
    /// The ticket of the last removal: a read of the projects taken before it is out of date.
    #[serde(skip)]
    pub(crate) removed_at: u64,
    /// The highest ticket of a projects read applied: an older read that ends later is stale.
    #[serde(skip)]
    pub(crate) read_at: u64,
}

impl PaletteLists {
    /// A place that left is retired, so a session list read before it left
    /// cannot bring its sessions back; one imported again is listed again.
    pub(crate) fn set_projects(&mut self, projects: Vec<ImportedProject>) {
        let current: BTreeSet<Place> = projects.iter().map(Place::from).collect();
        let left: Vec<Place> = self.projects.iter().flatten().map(Place::from).collect();
        self.retire(left);
        self.retired_places.retain(|place| !current.contains(place));
        self.forgotten_projects
            .extend(self.retired_places.iter().map(|p| p.project.clone()));

        for project in &projects {
            self.forgotten_projects.remove(&project.project);
        }

        self.projects = Some(projects);
    }

    /// Takes the imports a read at ticket `read` found, unless a removal or a newer read came
    /// after it, which the read may not have seen. True when the places differ from the ones held.
    pub(crate) fn read_projects(&mut self, read: u64, projects: Vec<ImportedProject>) -> bool {
        if read < self.removed_at || read < self.read_at {
            return false;
        }
        self.read_at = read;

        let places = |projects: &[ImportedProject]| -> BTreeSet<Place> {
            projects.iter().map(Place::from).collect()
        };
        let changed = places(self.projects.as_deref().unwrap_or_default()) != places(&projects);
        self.set_projects(projects);

        changed
    }

    pub(crate) fn forget_project(&mut self, project: &str) {
        self.forgotten_projects.insert(project.to_owned());
        let places: Vec<Place> = self
            .projects
            .iter()
            .flatten()
            .filter(|p| p.project == project)
            .map(Place::from)
            .collect();
        self.retire(places);
    }

    /// Adds `places` to the retired ones, each once.
    fn retire(&mut self, places: Vec<Place>) {
        let mut retired: BTreeSet<Place> = self.retired_places.drain(..).collect();
        retired.extend(places);
        self.retired_places = retired.into_iter().collect();
    }

    /// Takes out of `sessions` those in a place that left, which a read from before it left
    /// may still list.
    pub(crate) fn without_retired(&self, sessions: &mut Vec<MachineSession>) {
        let projects = self.projects.as_deref().unwrap_or_default();
        sessions.retain(|s| !retired(s, projects, &self.retired_places));
    }

    /// Keeps `session`, which the user opened, unless its place left.
    pub(crate) fn open(&mut self, session: MachineSession) {
        let projects = self.projects.as_deref().unwrap_or_default();
        if retired(&session, projects, &self.retired_places) {
            return;
        }

        self.opened
            .retain(|old| old.machine_id != session.machine_id || old.id != session.id);
        self.opened.push(session);
        let extra = self.opened.len().saturating_sub(OPENED_MOST);
        self.opened.drain(..extra);
    }

    pub(crate) fn prune(&mut self) {
        if let Some(projects) = &mut self.projects {
            projects.retain(|p| !self.forgotten_projects.contains(&p.project));
        }
        if let Some(branches) = &mut self.branches {
            branches.retain(|b| !self.forgotten_projects.contains(&b.project));
        }
    }
}

/// A branch with commits its project's default branch does not have.
#[derive(Serialize, Deserialize)]
pub(crate) struct BranchRow {
    pub(crate) project: String,
    pub(crate) name: String,
    pub(crate) ahead: u32,
    pub(crate) subject: String,
}

#[cfg(test)]
#[path = "../tests/palette_store.rs"]
mod tests;
