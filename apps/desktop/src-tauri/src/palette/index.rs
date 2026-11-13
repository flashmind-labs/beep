//! The palette's index: what each thing is called on screen and the
//! lowercase texts it is found by, group by group. `palette-rank.ts` ranks
//! it. Entry point: `build_index`.

use super::{
    lists::PaletteLists,
    projects::{project_entries, session_entries},
};
use crate::agents::sessions::SessionRow;
use serde::Serialize;
use strum::IntoEnumIterator;

/// A view the palette can go to without a search.
#[derive(Clone, Copy, strum::EnumIter, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum PaletteAction {
    Home,
    Settings,
    CodingAgents,
}

impl PaletteAction {
    fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Settings => "Settings",
            Self::CodingAgents => "Coding agents",
        }
    }
}

/// The groups of hits, in the order the palette lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HitKind {
    Action,
    Setting,
    /// Branches with work, under "Projects and code". The id is the project
    /// and the branch, split by a space.
    Branch,
    Project,
    /// A folder coding agents worked in, not imported. The id is its name.
    Folder,
    /// A coding agent's session, under "Coding agents".
    AgentSession,
    Machine,
}

/// A section of the Settings sheet, by the id the sheet's nav goes to.
#[derive(Clone, Copy, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum SettingsSection {
    Machines,
    Creds,
    General,
}

impl SettingsSection {
    fn title(self) -> &'static str {
        match self {
            Self::Machines => "Machines",
            Self::Creds => "Credentials",
            Self::General => "General",
        }
    }
}

/// Every setting the palette finds, with the section of the sheet that
/// holds it.
fn settings() -> impl Iterator<Item = (String, SettingsSection)> {
    use SettingsSection::*;
    let fixed = [
        ("Machines", Machines),
        ("GitHub token", Creds),
        ("Forgejo or Gitea token", Creds),
        ("Launch at login", General),
    ];
    fixed
        .into_iter()
        .map(|(title, section)| (title.to_owned(), section))
}

/// One thing the palette can find.
#[derive(Debug, Serialize)]
pub(crate) struct Entry {
    pub(super) kind: HitKind,
    pub(super) id: String,
    pub(super) title: String,
    pub(super) detail: String,
    /// A matching prompt, apart from the session's project and machine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) snippet: Option<String>,
    /// The lowercase texts it is found by.
    pub(super) keys: Vec<String>,
    /// The session an `AgentSession` entry opens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) session: Option<SessionRow>,
}

impl Entry {
    pub(super) fn new(
        kind: HitKind,
        id: String,
        title: String,
        detail: String,
        keys: &[&str],
    ) -> Self {
        Self {
            kind,
            id,
            title,
            detail,
            snippet: None,
            keys: keys.iter().map(|key| key.to_lowercase()).collect(),
            session: None,
        }
    }
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
        &[title],
    )
}

fn setting_entry(title: String, section: SettingsSection) -> Entry {
    let id = <&'static str>::from(section).to_owned();
    let detail = format!("Settings · {}", section.title());
    let keys = [title.as_str()];
    Entry::new(HitKind::Setting, id, title.clone(), detail, &keys)
}

/// The index of `lists`.
pub(crate) fn build_index(lists: &PaletteLists) -> Index {
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
        projects: project_entries(lists, imported),
        sessions: session_entries(lists, imported),
        machines: machines.collect(),
    }
}

#[cfg(test)]
#[path = "../tests/palette_index.rs"]
mod tests;
