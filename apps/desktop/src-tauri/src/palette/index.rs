//! The palette's index: what each thing is called on screen and the
//! lowercase texts it is found by, group by group. `palette-rank.ts` ranks
//! it. Entry points: `build_index` and the `center_find_text` command.

use super::{
    lists::PaletteLists,
    projects::{project_entries, session_entries},
};
use crate::{agents::sessions::SessionRow, beep::Service, center::section, error::Result};
use beep_proto::{ApiProvider, Session, SessionMatch};
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
        .chain(ApiProvider::all().map(|p| (format!("{} API key", p.name()), Creds)))
}

/// One thing the palette can find.
#[derive(Debug, Serialize)]
pub(crate) struct Entry {
    pub(super) kind: HitKind,
    pub(super) id: String,
    pub(super) title: String,
    pub(super) detail: String,
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
    Index {
        actions: PaletteAction::iter().map(action_entry).collect(),
        settings: settings().map(|(t, s)| setting_entry(t, s)).collect(),
        code: prs.chain(branches).collect(),
        projects: project_entries(lists, imported),
        sessions: session_entries(lists, imported),
        chats: chats.into_iter().map(chat_entry).collect(),
        memories: memories.collect(),
        routines: routines.collect(),
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
mod tests {
    use super::super::lists::{BranchRow, PrRow};
    use super::*;
    use beep_proto::{AgentStatus, ImportedProject, MachineSession, ProjectAgent};
    use std::time::{Duration, SystemTime};
    use uuid::Uuid;

    fn imported(project: &str, machine: &str) -> ImportedProject {
        ImportedProject {
            project: project.to_owned(),
            url: format!("https://example.com/{project}"),
            machine_id: machine.to_owned(),
            machine_name: machine.to_owned(),
            folder: "/srv/project".to_owned(),
            cloned: false,
            imported_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn pr(repo: &str, number: u64, title: &str) -> PrRow {
        PrRow {
            repo: repo.to_owned(),
            number,
            title: title.to_owned(),
            url: format!("https://example.com/{repo}/pull/{number}"),
            detail: "PR · yours · example.com".to_owned(),
        }
    }

    fn agent_session(
        id: &str,
        title: Option<&str>,
        cwd: &str,
        machine: &str,
        at: u64,
    ) -> MachineSession {
        MachineSession {
            machine_id: machine.to_owned(),
            machine_name: machine.to_owned(),
            id: id.to_owned(),
            agent: "claude".to_owned(),
            title: title.map(str::to_owned),
            cwd: cwd.to_owned(),
            status: AgentStatus::Working {},
            doing: None,
            started_at: SystemTime::UNIX_EPOCH,
            last_activity: SystemTime::UNIX_EPOCH + Duration::from_secs(at),
            external: false,
            question: None,
        }
    }

    fn board(name: &str, chat: u128) -> ProjectAgent {
        ProjectAgent {
            project_id: Uuid::from_u128(chat + 1000),
            name: name.to_owned(),
            chat_id: Uuid::from_u128(chat),
            open_tasks: 0,
            open_workers: 0,
        }
    }

    fn titles(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.title.as_str()).collect()
    }

    #[test]
    fn settings_carry_their_section() {
        let index = build_index(&PaletteLists::default());
        let key = index
            .settings
            .iter()
            .find(|e| e.title == "ElevenLabs API key")
            .expect("every API provider is a setting");
        assert_eq!(key.id, "creds");
        assert_eq!(key.detail, "Settings · Credentials");
        assert_eq!(key.keys, ["elevenlabs api key"]);
        let thinking = index
            .settings
            .iter()
            .filter(|e| e.keys[0].contains("thinking"));
        assert!(thinking.clone().count() > 1);
        assert!(thinking.clone().all(|e| e.id == "models"));
    }

    #[test]
    fn a_project_on_two_machines_is_listed_once() {
        let lists = PaletteLists {
            projects: Some(vec![
                imported("acme/widget", "build-box"),
                imported("acme/widget", "mac-box"),
                imported("acme/relay", "build-box"),
            ]),
            ..PaletteLists::default()
        };
        let projects = build_index(&lists).projects;
        assert_eq!(titles(&projects), ["acme/widget", "acme/relay"]);
        assert_eq!(projects[0].detail, "build-box");
        assert_eq!(projects[0].keys, ["acme/widget"]);
    }

    #[test]
    fn pull_requests_are_found_by_title_repo_or_number_then_branches() {
        let lists = PaletteLists {
            prs: Some(vec![pr("Acme/Widget", 42, "Fix the login loop")]),
            branches: Some(vec![BranchRow {
                project: "acme/widget".to_owned(),
                name: "fix-login".to_owned(),
                ahead: 3,
                subject: "Retry the token once".to_owned(),
            }]),
            ..PaletteLists::default()
        };
        let code = build_index(&lists).code;
        assert_eq!(code[0].id, "https://example.com/Acme/Widget/pull/42");
        assert_eq!(code[0].title, "Acme/Widget · PR #42 Fix the login loop");
        assert_eq!(code[0].keys, ["fix the login loop", "acme/widget", "#42"]);
        assert_eq!(code[1].kind, HitKind::Branch);
        assert_eq!(code[1].id, "acme/widget fix-login");
        assert_eq!(code[1].detail, "Branch · 3 ahead · Retry the token once");
    }

    #[test]
    fn a_folder_only_coding_agents_worked_in_is_a_project() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/relay", "build-box")]),
            sessions: Some(vec![
                agent_session("1", Some("Desktop UI"), "/work/widget", "build-box", 5),
                agent_session("2", None, "/work/widget", "mac-box", 9),
            ]),
            ..PaletteLists::default()
        };
        let projects = build_index(&lists).projects;
        assert_eq!(titles(&projects), ["acme/relay", "widget"]);
        assert_eq!(projects[1].id, "widget");
        assert_eq!(
            projects[1].detail,
            "Not imported · 2 sessions · build-box, mac-box"
        );
    }

    #[test]
    fn a_board_project_that_is_not_imported_opens_its_chat() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/relay", "build-box")]),
            boards: Some(vec![board("acme/relay", 1), board("acme/widget", 2)]),
            ..PaletteLists::default()
        };
        let projects = build_index(&lists).projects;
        assert_eq!(titles(&projects), ["acme/relay", "acme/widget"]);
        assert_eq!(projects[1].id, Uuid::from_u128(2).to_string());
    }

    #[test]
    fn projects_are_listed_once_across_lists() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/relay", "build-box")]),
            boards: Some(vec![board("acme/relay", 1), board("acme/widget", 2)]),
            sessions: Some(vec![
                // Inside acme/relay's folder on its machine.
                agent_session("1", None, "/srv/project/sub", "build-box", 1),
                // A checkout named like the board's acme/widget.
                agent_session("2", None, "/home/user/Widget", "mac-box", 2),
            ]),
            ..PaletteLists::default()
        };
        let projects = build_index(&lists).projects;
        assert_eq!(titles(&projects), ["acme/relay", "acme/widget"]);
    }

    #[test]
    fn coding_agents_are_found_by_title_project_harness_and_machine() {
        let lists = PaletteLists {
            sessions: Some(vec![agent_session(
                "s1",
                Some("Cross-machine desktop UI"),
                "/srv/project/widget",
                "build-box",
                1,
            )]),
            ..PaletteLists::default()
        };
        let sessions = build_index(&lists).sessions;
        assert_eq!(sessions[0].title, "Cross-machine desktop UI");
        assert_eq!(sessions[0].detail, "Claude Code · widget · build-box");
        assert_eq!(
            sessions[0].keys,
            [
                "cross-machine desktop ui",
                "widget",
                "claude code",
                "claude",
                "build-box"
            ]
        );
        assert!(sessions[0].session.is_some());
    }

    #[test]
    fn coding_agents_and_chats_list_newest_first() {
        let session = |id: &str, updated_at| Session {
            id: id.to_owned(),
            title: id.to_owned(),
            pinned: id == "old",
            updated_at,
        };
        let lists = PaletteLists {
            sessions: Some(vec![
                agent_session("a", Some("Old"), "/srv/a", "build-box", 1),
                agent_session("b", Some("New"), "/srv/b", "build-box", 9),
            ]),
            chats: Some(vec![session("old", 1), session("new", 5)]),
            ..PaletteLists::default()
        };
        let index = build_index(&lists);
        assert_eq!(titles(&index.sessions), ["New", "Old"]);
        assert_eq!(titles(&index.chats), ["new", "old"]);
    }

    #[test]
    fn an_untitled_session_is_named_by_its_project() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/widget", "build-box")]),
            sessions: Some(vec![agent_session(
                "s1",
                None,
                "/srv/project/src",
                "build-box",
                1,
            )]),
            ..PaletteLists::default()
        };
        let index = build_index(&lists);
        assert_eq!(index.sessions[0].title, "acme/widget");
        assert_eq!(
            index.sessions[0].detail,
            "Claude Code · acme/widget · build-box"
        );
        assert_eq!(titles(&index.projects), ["acme/widget"]);
    }
}
