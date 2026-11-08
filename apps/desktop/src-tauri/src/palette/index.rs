//! The palette's index: what each thing is called on screen and the
//! lowercase texts it is found by, group by group. `palette-rank.ts` ranks
//! it. Entry points: `build_index` and the `center_find_text` command.

use super::{
    lists::PaletteLists,
    projects::{project_entries, session_entries},
};
use crate::{agents::sessions::SessionRow, beep::Service, center::section, error::Result};
use beep_proto::{Session, SessionMatch};
use serde::Serialize;
use std::cmp::Reverse;
use strum::IntoEnumIterator;

/// Chat text is searched from this many characters.
const MIN_TEXT_QUERY: usize = 2;

/// A view the palette can go to without a search.
#[derive(Clone, Copy, strum::EnumIter, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum PaletteAction {
    NewChat,
    Home,
    AllChats,
    Memories,
    Routines,
    Skills,
    Apps,
    Settings,
    CodingAgents,
}

impl PaletteAction {
    /// Whether it opens something only a subscription has.
    fn needs_subscription(self) -> bool {
        matches!(
            self,
            Self::NewChat
                | Self::AllChats
                | Self::Memories
                | Self::Routines
                | Self::Skills
                | Self::Apps
        )
    }

    fn title(self) -> &'static str {
        match self {
            Self::NewChat => "New chat",
            Self::Home => "Home",
            Self::AllChats => "All chats",
            Self::Memories => "Memories",
            Self::Routines => "Routines",
            Self::Skills => "Skills",
            Self::Apps => "Apps",
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
    /// Pull requests, under "Projects and code".
    Code,
    /// Branches with work, also under "Projects and code". The id is the
    /// project and the branch, split by a space.
    Branch,
    Project,
    /// A project on the board that is not imported. The id is its chat.
    ProjectChat,
    /// A folder coding agents worked in, not imported. The id is its name.
    Folder,
    /// A coding agent's session, under "Coding agents".
    AgentSession,
    Chat,
    Memory,
    Routine,
    Machine,
}

/// A section of the Settings sheet, by the id the sheet's nav goes to.
#[derive(Clone, Copy, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum SettingsSection {
    Devices,
    Models,
    Apps,
    Mac,
    Machines,
    Agents,
    Creds,
    General,
}

impl SettingsSection {
    /// Whether the page is only there with a subscription.
    fn needs_subscription(self) -> bool {
        matches!(
            self,
            Self::Devices | Self::Models | Self::Apps | Self::Mac | Self::Agents
        )
    }

    fn title(self) -> &'static str {
        match self {
            Self::Devices => "Account",
            Self::Models => "Models",
            Self::Apps => "Apps",
            Self::Mac => "This Mac",
            Self::Machines => "Machines",
            Self::Agents => "Agent accounts",
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
        ("Account", Devices),
        ("Devices", Devices),
        ("Unlink this Mac", Devices),
        ("Model", Models),
        ("Thinking", Models),
        ("Auto thinking", Models),
        ("Apps", Apps),
        ("Let Beep use this Mac", Mac),
        ("Machines", Machines),
        ("Agent accounts", Agents),
        ("Claude Code token", Agents),
        ("Codex sign-in", Agents),
        ("GitHub token", Creds),
        ("Forgejo or Gitea token", Creds),
        ("Launch at login", General),
        ("Voice notes", General),
        ("When I stop talking", General),
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
    /// Newest first.
    chats: Vec<Entry>,
    memories: Vec<Entry>,
    routines: Vec<Entry>,
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

fn chat_entry(session: &Session) -> Entry {
    let (id, title) = (session.id.clone(), session.title.clone());
    Entry::new(HitKind::Chat, id, title, String::new(), &[&session.title])
}

/// A chat whose messages hold what was typed, with the snippet that does.
fn text_entry(found: SessionMatch) -> Entry {
    let SessionMatch {
        session, snippet, ..
    } = found;
    Entry::new(HitKind::Chat, session.id, session.title, snippet, &[])
}

/// The index of `lists`.
pub(crate) fn build_index(lists: &PaletteLists) -> Index {
    let imported = lists.projects.as_deref().unwrap_or_default();
    let prs = lists.prs.iter().flatten().map(|pr| {
        let title = format!("{} · PR #{} {}", pr.repo, pr.number, pr.title);
        let number = format!("#{}", pr.number);
        let keys = [pr.title.as_str(), &pr.repo, &number];
        Entry::new(
            HitKind::Code,
            pr.url.clone(),
            title,
            pr.detail.clone(),
            &keys,
        )
    });
    let branches = lists.branches.iter().flatten().map(|b| {
        Entry::new(
            HitKind::Branch,
            format!("{} {}", b.project, b.name),
            format!("{} · {}", b.project, b.name),
            format!("Branch · {} ahead · {}", b.ahead, b.subject),
            &[&b.name, &b.project],
        )
    });
    let mut chats: Vec<&Session> = lists.chats.iter().flatten().collect();
    chats.sort_by_key(|s| Reverse(s.updated_at));
    let memories = lists.memories.iter().flatten().map(|m| {
        let (id, content) = (m.id.clone(), m.content.clone());
        Entry::new(
            HitKind::Memory,
            id,
            content,
            m.category.clone(),
            &[&m.content],
        )
    });
    let routines = lists.routines.iter().flatten().map(|r| {
        let (id, label) = (r.id.clone(), r.label.clone());
        Entry::new(HitKind::Routine, id, label, r.schedule.clone(), &[&r.label])
    });
    let machines = lists.machines.iter().flatten().map(|m| {
        let state = if m.online { "online" } else { "offline" };
        let (id, name) = (m.id.clone(), m.name.clone());
        Entry::new(HitKind::Machine, id, name, state.to_owned(), &[&m.name])
    });
    let local = lists.without_subscription;
    Index {
        actions: PaletteAction::iter()
            .filter(|action| !(local && action.needs_subscription()))
            .map(action_entry)
            .collect(),
        settings: settings()
            .filter(|(_, section)| !(local && section.needs_subscription()))
            .map(|(t, s)| setting_entry(t, s))
            .collect(),
        code: prs.chain(branches).collect(),
        projects: project_entries(lists, imported),
        sessions: session_entries(lists, imported),
        chats: chats
            .into_iter()
            .map(chat_entry)
            .filter(|_| !local)
            .collect(),
        memories: memories.filter(|_| !local).collect(),
        routines: routines.filter(|_| !local).collect(),
        machines: machines.collect(),
    }
}

/// The chats whose messages hold `query`. The window asks one at a time.
#[tauri::command]
pub async fn center_find_text(beep: Service<'_>, query: String) -> Result<Vec<Entry>> {
    let query = query.trim().to_owned();
    if query.chars().count() < MIN_TEXT_QUERY {
        return Ok(Vec::new());
    }
    let found = section("chat text", beep.search(query).await).unwrap_or_default();
    Ok(found.into_iter().map(text_entry).collect())
}

#[cfg(test)]
#[path = "../tests/palette_index.rs"]
mod tests;
