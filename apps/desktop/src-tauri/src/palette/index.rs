//! The palette's index: what each thing is called on screen and the
//! lowercase texts it is found by, group by group. `palette-rank.ts` ranks
//! it. Entry point: `build_index`.

use super::projects::{Entry, HitKind, project_entries, session_entries};
use crate::palette_store::lists::PaletteLists;
use beep_proto::MachineSession;
use serde::Serialize;
use strum::IntoEnumIterator;

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
