//! The palette's index: what each thing is called on screen and the
//! lowercase texts it is found by, group by group. `palette-rank.ts` ranks
//! it. Entry point: `build_index`.

use super::{
    places::{Place, retired},
    projects::{Entry, HitKind, project_entries, session_entries},
};
use beep_proto::{ImportedProject, Machine, MachineSession};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use strum::IntoEnumIterator;

/// The most sessions kept that the user opened and the mirror does not hold.
pub(super) const OPENED_MOST: usize = 50;

/// A view or window command the palette can run without a search.
#[derive(Clone, Copy, strum::EnumIter, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum PaletteAction {
    NewSession,
    Home,
    Projects,
    Terminals,
    AddProject,
    JoinTeam,
    AddMachine,
    Settings,
    CodingAgents,
    Shortcuts,
    ReportProblem,
}

impl PaletteAction {
    fn title(self) -> &'static str {
        match self {
            Self::NewSession => "New session",
            Self::Home => "Home",
            Self::Projects => "Projects",
            Self::Terminals => "Terminals",
            Self::AddProject => "Add project",
            Self::JoinTeam => "Join team",
            Self::AddMachine => "Add machine",
            Self::Settings => "Settings",
            Self::CodingAgents => "Coding agents",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::ReportProblem => "Report a problem",
        }
    }

    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::NewSession => &["New session", "Start an agent", "New agent"],
            Self::Home => &["Home", "Board", "Review"],
            Self::Projects => &["Projects", "Repositories"],
            Self::Terminals => &["Terminals", "Shell"],
            Self::AddProject => &["Add project", "Import project", "Clone repository"],
            Self::JoinTeam => &["Join team", "Team invite", "Invite code"],
            Self::AddMachine => &["Add machine", "Pair machine", "Link machine"],
            Self::Settings => &["Settings", "Preferences"],
            Self::CodingAgents => &["Coding agents", "Harnesses", "Providers"],
            Self::Shortcuts => &["Keyboard shortcuts", "Keys", "Hotkeys"],
            Self::ReportProblem => &["Report a problem", "Bug report", "Logs"],
        }
    }
}

/// A section of the Settings sheet, by the id the sheet's nav goes to.
#[derive(Clone, Copy, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum SettingsSection {
    Machines,
    Creds,
    General,
    Appearance,
    Instr,
    Plugins,
}

impl SettingsSection {
    fn title(self) -> &'static str {
        match self {
            Self::Machines => "Machines",
            Self::Creds => "Credentials",
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Instr => "Instructions",
            Self::Plugins => "Plugins",
        }
    }
}

/// Every setting the palette finds, with the section of the sheet that
/// holds it.
fn settings() -> impl Iterator<Item = (String, SettingsSection)> {
    use SettingsSection::*;
    let fixed = [
        ("Machines", Machines),
        ("Appearance and skin", Appearance),
        ("Instructions", Instr),
        ("Plugins", Plugins),
        ("GitHub token", Creds),
        ("Forgejo or Gitea token", Creds),
        ("Launch at login", General),
    ];
    fixed
        .into_iter()
        .map(|(title, section)| (title.to_owned(), section))
}

/// The lists the index is built from. A list stays `None` until it was read
/// once; after that the last answer stays when a read fails.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct PaletteLists {
    pub(super) machines: Option<Vec<Machine>>,
    pub(super) projects: Option<Vec<ImportedProject>>,
    #[serde(default)]
    pub(super) retired_places: Vec<Place>,
    #[serde(default)]
    pub(super) forgotten_projects: HashSet<String>,
    pub(super) branches: Option<Vec<BranchRow>>,
    /// Sessions the user opened that the mirror did not hold, such as one only in a project's
    /// history, newest last.
    #[serde(default)]
    pub(super) opened: Vec<MachineSession>,
    /// The ticket of the last removal: a read of the projects taken before it is out of date.
    #[serde(skip)]
    pub(super) removed_at: u64,
    /// The highest ticket of a projects read applied: an older read that ends later is stale.
    #[serde(skip)]
    pub(super) read_at: u64,
}

impl PaletteLists {
    /// A place that left is retired, so a session list read before it left
    /// cannot bring its sessions back; one imported again is listed again.
    pub(super) fn set_projects(&mut self, projects: Vec<ImportedProject>) {
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
    pub(super) fn read_projects(&mut self, read: u64, projects: Vec<ImportedProject>) -> bool {
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

    pub(super) fn forget_project(&mut self, project: &str) {
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
    pub(super) fn without_retired(&self, sessions: &mut Vec<MachineSession>) {
        let projects = self.projects.as_deref().unwrap_or_default();
        sessions.retain(|s| !retired(s, projects, &self.retired_places));
    }

    /// Keeps `session`, which the user opened, unless its place left.
    pub(super) fn open(&mut self, session: MachineSession) {
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

    pub(super) fn prune(&mut self) {
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
    pub(super) project: String,
    pub(super) name: String,
    pub(super) ahead: u32,
    pub(super) subject: String,
}

/// Everything the palette finds, a list per group, each in the order its
/// ties list.
#[derive(Debug, Serialize)]
pub(crate) struct Index {
    actions: Vec<Entry>,
    settings: Vec<Entry>,
    code: Vec<Entry>,
    projects: Vec<Entry>,
    sessions: Vec<Entry>,
    machines: Vec<Entry>,
}

fn action_entry(action: PaletteAction) -> Entry {
    let (id, title) = (<&'static str>::from(action), action.title());
    Entry::new(
        HitKind::Action,
        id.into(),
        title.into(),
        String::new(),
        action.keys(),
    )
}

fn setting_entry(title: String, section: SettingsSection) -> Entry {
    let id = <&'static str>::from(section).to_owned();
    let detail = format!("Settings · {}", section.title());
    let keys = [title.as_str()];
    Entry::new(HitKind::Setting, id, title.clone(), detail, &keys)
}

/// The index of `lists` and the `sessions` of the machines.
pub(crate) fn build_index(lists: &PaletteLists, sessions: &[MachineSession]) -> Index {
    let imported = lists.projects.as_deref().unwrap_or_default();
    let branches = lists.branches.iter().flatten().map(|b| {
        Entry::new(
            HitKind::Branch,
            format!("{} {}", b.project, b.name),
            format!("{} · {}", b.project, b.name),
            format!("Branch · {} ahead · {}", b.ahead, b.subject),
            &[&b.name, &b.project],
        )
    });
    let machines = lists.machines.iter().flatten().map(|m| {
        let state = if m.online { "online" } else { "offline" };
        let (id, name) = (m.id.clone(), m.name.clone());
        Entry::new(HitKind::Machine, id, name, state.to_owned(), &[&m.name])
    });
    Index {
        actions: PaletteAction::iter().map(action_entry).collect(),
        settings: settings().map(|(t, s)| setting_entry(t, s)).collect(),
        code: branches.collect(),
        projects: project_entries(sessions, imported),
        sessions: session_entries(sessions, imported),
        machines: machines.collect(),
    }
}

#[cfg(test)]
#[path = "../tests/palette_index.rs"]
mod tests;
