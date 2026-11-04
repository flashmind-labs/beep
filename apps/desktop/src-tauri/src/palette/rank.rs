//! Ranking what was typed against the palette's lists: a tier per match,
//! the best few of each kind, and one top hit. Entry points: `rank` and the
//! `center_find` command.

use super::{
    lists::{PaletteCache, PaletteLists},
    projects::{project_hits, session_hits},
};
use crate::{agents::sessions::SessionRow, beep::Service, center::section, error::Result};
use beep_proto::{ApiProvider, Session, SessionMatch};
use serde::Serialize;
use std::{cmp::Reverse, collections::HashMap};
use strum::IntoEnumIterator;
use tauri::State;
use tokio::join;

/// Most hits of one kind.
pub(super) const PER_KIND: usize = 8;
/// Chats listed before anything is typed.
const RECENT_CHATS: usize = 5;
/// Chat text is searched from this many characters.
const MIN_TEXT_QUERY: usize = 2;

/// Chats for one query: every chat (matched by title) and the chats whose
/// messages hold the words.
pub(crate) struct ChatHits {
    titles: Vec<Session>,
    text: Vec<SessionMatch>,
}

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

#[derive(Debug, Serialize)]
pub(crate) struct Hit {
    pub(super) kind: HitKind,
    id: String,
    title: String,
    detail: String,
    /// The one best hit, listed first under Top hit instead of its group.
    top: bool,
    /// The session an `AgentSession` hit opens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) session: Option<SessionRow>,
}

impl Hit {
    pub(super) fn new(kind: HitKind, id: String, title: String, detail: String) -> Self {
        Self {
            kind,
            id,
            title,
            detail,
            top: false,
            session: None,
        }
    }
}

/// How well a text matches, best first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Tier {
    Prefix,
    WordStart,
    Substring,
    /// Only found in a chat's messages.
    Text,
}

/// `query` must be lowercase.
pub(super) fn tier(query: &str, text: &str) -> Option<Tier> {
    let text = text.to_lowercase();
    if text.starts_with(query) {
        return Some(Tier::Prefix);
    }
    let at_word_start = |at: usize| {
        text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
    };
    let mut found = text.match_indices(query).map(|(at, _)| at).peekable();
    found.peek()?;
    Some(if found.any(at_word_start) {
        Tier::WordStart
    } else {
        Tier::Substring
    })
}

/// Keeps the best `PER_KIND` of the matching hits, ties in list order.
pub(super) fn best(mut matched: Vec<(Tier, Hit)>) -> Vec<(Tier, Hit)> {
    matched.sort_by_key(|(tier, _)| *tier);
    matched.truncate(PER_KIND);
    matched
}

/// The groups in the order the palette lists them, with the top hit
/// first: the best hit that starts with what was typed, when hits come
/// from more than one group. Groups earlier in `TOP_ORDER` win a tie.
fn with_top(mut groups: Vec<Vec<(Tier, Hit)>>) -> Vec<Hit> {
    let filled = groups.iter().filter(|group| !group.is_empty()).count();
    let top = (filled > 1)
        .then(|| {
            TOP_ORDER.iter().find_map(|kind| {
                groups.iter().position(|group| {
                    group
                        .first()
                        .is_some_and(|(tier, hit)| *tier == Tier::Prefix && hit.kind == *kind)
                })
            })
        })
        .flatten();
    let mut out = Vec::new();
    if let Some(at) = top {
        let (_, mut hit) = groups[at].remove(0);
        hit.top = true;
        out.push(hit);
    }
    out.extend(groups.into_iter().flatten().map(|(_, hit)| hit));
    out
}

/// Which group's best hit becomes the top hit on a tie: the things a
/// search most often means first.
const TOP_ORDER: [HitKind; 12] = [
    HitKind::Project,
    HitKind::ProjectChat,
    HitKind::Folder,
    HitKind::Machine,
    HitKind::Code,
    HitKind::Branch,
    HitKind::AgentSession,
    HitKind::Setting,
    HitKind::Chat,
    HitKind::Routine,
    HitKind::Memory,
    HitKind::Action,
];

fn chat_hit(session: &Session, snippet: &str) -> Hit {
    let (id, title) = (session.id.clone(), session.title.clone());
    Hit::new(HitKind::Chat, id, title, snippet.to_owned())
}

fn action_hit(action: PaletteAction) -> Hit {
    let (id, title) = (<&'static str>::from(action), action.title());
    Hit::new(HitKind::Action, id.into(), title.into(), String::new())
}

/// A chat found by title and by text is one hit, with the snippet.
fn chat_hits(query: &str, chats: &ChatHits) -> Vec<(Tier, Hit)> {
    struct Found<'a> {
        tier: Tier,
        session: &'a Session,
        snippet: &'a str,
    }
    let mut found: Vec<Found> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for session in &chats.titles {
        if let Some(tier) = tier(query, &session.title) {
            at.insert(&session.id, found.len());
            found.push(Found {
                tier,
                session,
                snippet: "",
            });
        }
    }
    for m in &chats.text {
        match at.get(m.session.id.as_str()) {
            Some(&i) => {
                if found[i].snippet.is_empty() {
                    found[i].snippet = &m.snippet;
                }
            }
            None => {
                at.insert(&m.session.id, found.len());
                found.push(Found {
                    tier: Tier::Text,
                    session: &m.session,
                    snippet: &m.snippet,
                });
            }
        }
    }
    found.sort_by_key(|f| (f.tier, Reverse(f.session.updated_at)));
    found
        .into_iter()
        .take(PER_KIND)
        .map(|f| (f.tier, chat_hit(f.session, f.snippet)))
        .collect()
}

/// Hits for `query`, a group at a time. Nothing typed lists every action
/// and the newest chats.
pub(crate) fn rank(query: &str, lists: &PaletteLists, chats: &ChatHits) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        let mut recent: Vec<&Session> = chats.titles.iter().collect();
        recent.sort_by_key(|s| Reverse(s.updated_at));
        return PaletteAction::iter()
            .map(action_hit)
            .chain(
                recent
                    .into_iter()
                    .take(RECENT_CHATS)
                    .map(|s| chat_hit(s, "")),
            )
            .collect();
    }
    let actions = PaletteAction::iter()
        .filter_map(|a| Some((tier(&query, a.title())?, action_hit(a))))
        .collect();
    let memories = lists.memories.iter().flatten().filter_map(|m| {
        let (id, content) = (m.id().to_owned(), m.content().to_owned());
        let hit = Hit::new(HitKind::Memory, id, content, m.category().to_owned());
        Some((tier(&query, m.content())?, hit))
    });
    let routines = lists.routines.iter().flatten().filter_map(|r| {
        let (id, label) = (r.id.clone(), r.label.clone());
        let hit = Hit::new(HitKind::Routine, id, label, r.schedule.clone());
        Some((tier(&query, &r.label)?, hit))
    });
    let machines = lists.machines.iter().flatten().filter_map(|m| {
        let state = if m.online() { "online" } else { "offline" };
        let (id, name) = (m.id().to_owned(), m.name().to_owned());
        let hit = Hit::new(HitKind::Machine, id, name, state.to_owned());
        Some((tier(&query, m.name())?, hit))
    });
    let settings = settings().filter_map(|(title, section)| {
        let tier = tier(&query, &title)?;
        let id = <&'static str>::from(section).to_owned();
        let detail = format!("Settings · {}", section.title());
        Some((tier, Hit::new(HitKind::Setting, id, title, detail)))
    });
    let imported = lists.projects.as_deref().unwrap_or_default();
    let projects = project_hits(&query, lists, imported);
    let sessions = session_hits(&query, lists, imported);
    let code = lists.prs.iter().flatten().filter_map(|pr| {
        let title = format!("{} · PR #{} {}", pr.repo, pr.number, pr.title);
        let tier = tier(&query, &pr.title)
            .or_else(|| tier(&query, &pr.repo))
            .or_else(|| tier(&query, &format!("#{}", pr.number)))?;
        let hit = Hit::new(HitKind::Code, pr.url.clone(), title, pr.detail.clone());
        Some((tier, hit))
    });
    let branches = lists.branches.iter().flatten().filter_map(|b| {
        let tier = tier(&query, &b.name).or_else(|| tier(&query, &b.project))?;
        let hit = Hit::new(
            HitKind::Branch,
            format!("{} {}", b.project, b.name),
            format!("{} · {}", b.project, b.name),
            format!("Branch · {} ahead · {}", b.ahead, b.subject),
        );
        Some((tier, hit))
    });
    let code = code.chain(branches);
    with_top(vec![
        best(actions),
        best(settings.collect()),
        best(code.collect()),
        projects,
        sessions,
        chat_hits(&query, chats),
        best(memories.collect()),
        best(routines.collect()),
        best(machines.collect()),
    ])
}

/// Ranks the hits for `query`. Without `text` it reads only what the app
/// holds (the palette's lists and the local copy of the chats), so it
/// answers at once on every keystroke. With `text` it also searches chat
/// messages, and asks the server for what the copy does not hold yet.
#[tauri::command]
pub async fn center_find(
    beep: Service<'_>,
    cache: State<'_, PaletteCache>,
    query: String,
    text: bool,
) -> Result<Vec<Hit>> {
    let chats = if text {
        let wants_text = query.trim().chars().count() >= MIN_TEXT_QUERY;
        let (titles, text) = join!(beep.sessions(), async {
            if wants_text {
                Some(beep.search(query.trim().to_owned()).await)
            } else {
                None
            }
        });
        ChatHits {
            titles: section("chats", titles).unwrap_or_default(),
            text: text
                .and_then(|found| section("chat text", found))
                .unwrap_or_default(),
        }
    } else {
        ChatHits {
            titles: beep.chats().sessions().unwrap_or_default(),
            text: Vec::new(),
        }
    };
    Ok(rank(&query, &cache.lists(), &chats))
}

#[cfg(test)]
mod tests {
    use super::super::lists::{BranchRow, PrRow};
    use super::*;
    use crate::{machines::list::MachineView, settings::memories::MemoryView};
    use beep_proto::{
        AgentStatus, Cron, ImportedProject, Machine, MachineSession, Memory, MemorySource,
        ProjectAgent,
    };
    use std::time::{Duration, SystemTime};
    use uuid::Uuid;

    fn routine(label: &str) -> Cron {
        Cron {
            id: label.to_owned(),
            label: label.to_owned(),
            prompt: String::new(),
            schedule: "0 8 * * *".to_owned(),
            enabled: true,
            created_at: String::new(),
            run_once: false,
            last_run_at: None,
            next_run_at: None,
        }
    }

    fn memory(content: &str) -> MemoryView {
        MemoryView::from(Memory {
            id: content.to_owned(),
            content: content.to_owned(),
            category: "facts".to_owned(),
            source: MemorySource::User {},
            updated: "2026-01-01".to_owned(),
            valid_from: None,
            valid_until: None,
            quote: None,
        })
    }

    fn machine(name: &str) -> MachineView {
        MachineView::from(Machine {
            id: name.to_owned(),
            name: name.to_owned(),
            online: true,
            seen_at: SystemTime::UNIX_EPOCH,
            version: String::new(),
            env: None,
        })
    }

    fn session(id: &str, title: &str, updated_at: i64) -> Session {
        Session {
            id: id.to_owned(),
            title: title.to_owned(),
            pinned: false,
            updated_at,
        }
    }

    fn none() -> ChatHits {
        ChatHits {
            titles: Vec::new(),
            text: Vec::new(),
        }
    }

    fn of_kind(hits: &[Hit], kind: HitKind) -> Vec<&Hit> {
        hits.iter().filter(|h| h.kind == kind).collect()
    }

    #[test]
    fn prefix_beats_substring() {
        let lists = PaletteLists {
            routines: Some(vec![routine("Morning brief"), routine("Brief review")]),
            ..PaletteLists::default()
        };
        let hits = rank("bri", &lists, &none());
        let routines = of_kind(&hits, HitKind::Routine);
        assert_eq!(routines[0].title, "Brief review");
        assert_eq!(routines[1].title, "Morning brief");
    }

    #[test]
    fn word_start_beats_substring() {
        let lists = PaletteLists {
            routines: Some(vec![routine("Prebrief"), routine("Morning brief")]),
            ..PaletteLists::default()
        };
        let hits = rank("bri", &lists, &none());
        let routines = of_kind(&hits, HitKind::Routine);
        assert_eq!(routines[0].title, "Morning brief");
        assert_eq!(routines[1].title, "Prebrief");
    }

    #[test]
    fn title_match_beats_text_only_match() {
        let text_only = session("a", "Groceries", 9);
        let chats = ChatHits {
            titles: vec![text_only.clone(), session("b", "Trip plan", 1)],
            text: vec![SessionMatch {
                session: text_only,
                snippet: "...trip...".to_owned(),
                from_user: true,
            }],
        };
        let hits = rank("trip", &PaletteLists::default(), &chats);
        let ids: Vec<&str> = of_kind(&hits, HitKind::Chat)
            .iter()
            .map(|h| h.id.as_str())
            .collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn equal_tiers_list_newest_first() {
        let chats = ChatHits {
            titles: vec![
                session("old", "Tea one", 1),
                session("new", "Tea two", 5),
                session("mid", "Tea three", 3),
            ],
            text: Vec::new(),
        };
        let hits = rank("tea", &PaletteLists::default(), &chats);
        let ids: Vec<&str> = of_kind(&hits, HitKind::Chat)
            .iter()
            .map(|h| h.id.as_str())
            .collect();
        assert_eq!(ids, ["new", "mid", "old"]);
    }

    #[test]
    fn first_text_snippet_is_kept() {
        let found = session("a", "Trip plan", 1);
        let m = |snippet: &str| SessionMatch {
            session: found.clone(),
            snippet: snippet.to_owned(),
            from_user: true,
        };
        let chats = ChatHits {
            titles: vec![found.clone()],
            text: vec![m("first"), m("second")],
        };
        let hits = rank("trip", &PaletteLists::default(), &chats);
        assert_eq!(of_kind(&hits, HitKind::Chat)[0].detail, "first");
    }

    #[test]
    fn chat_found_twice_shows_once() {
        let found = session("a", "Trip plan", 1);
        let chats = ChatHits {
            titles: vec![found.clone()],
            text: vec![SessionMatch {
                session: found,
                snippet: "...a trip to Lisbon...".to_owned(),
                from_user: true,
            }],
        };
        let hits = rank("trip", &PaletteLists::default(), &chats);
        let chats = of_kind(&hits, HitKind::Chat);
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].detail, "...a trip to Lisbon...");
    }

    #[test]
    fn caps_each_kind_at_eight() {
        let lists = PaletteLists {
            memories: Some((0..20).map(|i| memory(&format!("likes tea {i}"))).collect()),
            ..PaletteLists::default()
        };
        let hits = rank("tea", &lists, &none());
        assert_eq!(of_kind(&hits, HitKind::Memory).len(), 8);
    }

    #[test]
    fn empty_query_lists_actions_then_recent_chats() {
        let chats = ChatHits {
            titles: (0..7)
                .map(|i| session(&i.to_string(), &format!("chat {i}"), i))
                .collect(),
            text: Vec::new(),
        };
        let hits = rank("", &PaletteLists::default(), &chats);
        assert_eq!(hits.len(), 14);
        assert!(hits[..9].iter().all(|h| h.kind == HitKind::Action));
        let ids: Vec<&str> = hits[9..].iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["6", "5", "4", "3", "2"]);
    }

    #[test]
    fn failed_lists_hide_their_group() {
        let chats = ChatHits {
            titles: vec![session("a", "Tea notes", 1)],
            text: Vec::new(),
        };
        let lists = PaletteLists {
            memories: None,
            machines: Some(vec![machine("tea-box")]),
            ..PaletteLists::default()
        };
        let hits = rank("tea", &lists, &chats);
        assert!(of_kind(&hits, HitKind::Memory).is_empty());
        assert_eq!(of_kind(&hits, HitKind::Chat).len(), 1);
        assert_eq!(of_kind(&hits, HitKind::Machine).len(), 1);
    }

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

    #[test]
    fn settings_are_found_with_their_section() {
        let hits = rank("elevenlabs", &PaletteLists::default(), &none());
        let settings = of_kind(&hits, HitKind::Setting);
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].title, "ElevenLabs API key");
        assert_eq!(settings[0].id, "creds");
        assert_eq!(settings[0].detail, "Settings · Credentials");
        let hits = rank("thinking", &PaletteLists::default(), &none());
        let models = of_kind(&hits, HitKind::Setting);
        assert!(
            models.iter().all(|hit| hit.id == "models"),
            "{:?}",
            models.iter().map(|hit| &hit.title).collect::<Vec<_>>()
        );
        assert_eq!(models[0].detail, "Settings · Models");
    }

    #[test]
    fn a_project_on_two_machines_is_found_once() {
        let lists = PaletteLists {
            projects: Some(vec![
                imported("acme/widget", "build-box"),
                imported("acme/widget", "mac-box"),
                imported("acme/relay", "build-box"),
            ]),
            ..PaletteLists::default()
        };
        let hits = rank("widget", &lists, &none());
        let projects = of_kind(&hits, HitKind::Project);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, "acme/widget");
        assert_eq!(projects[0].detail, "build-box");
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

    #[test]
    fn pull_requests_are_found_by_title_repo_or_number() {
        let lists = PaletteLists {
            prs: Some(vec![
                pr("acme/widget", 42, "Fix the login loop"),
                pr("acme/relay", 7, "Bump deps"),
            ]),
            ..PaletteLists::default()
        };
        for query in ["login", "widget", "#42"] {
            let hits = rank(query, &lists, &none());
            let code = of_kind(&hits, HitKind::Code);
            assert_eq!(code.len(), 1, "{query}");
            assert_eq!(code[0].id, "https://example.com/acme/widget/pull/42");
            assert_eq!(code[0].title, "acme/widget · PR #42 Fix the login loop");
        }
    }

    #[test]
    fn branches_are_found_with_pull_requests() {
        let lists = PaletteLists {
            prs: Some(vec![pr("acme/widget", 42, "Fix the login loop")]),
            branches: Some(vec![BranchRow {
                project: "acme/widget".to_owned(),
                name: "fix-login".to_owned(),
                ahead: 3,
                subject: "Retry the token once".to_owned(),
            }]),
            ..PaletteLists::default()
        };
        let hits = rank("fix", &lists, &none());
        let branch = of_kind(&hits, HitKind::Branch);
        assert_eq!(branch.len(), 1);
        assert_eq!(branch[0].id, "acme/widget fix-login");
        assert_eq!(branch[0].detail, "Branch · 3 ahead · Retry the token once");
        let code = hits
            .iter()
            .position(|h| h.kind == HitKind::Code)
            .expect("the PR matches");
        let at = hits
            .iter()
            .position(|h| h.kind == HitKind::Branch)
            .expect("the branch matches");
        assert_eq!(at, code + 1, "branches follow pull requests in one group");
    }

    #[test]
    fn top_hit_leads_when_several_groups_match() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/widget", "build-box")]),
            prs: Some(vec![pr("acme/widget", 42, "Widget polish")]),
            ..PaletteLists::default()
        };
        let chats = ChatHits {
            titles: vec![session("a", "Widget ideas", 1)],
            text: Vec::new(),
        };
        let hits = rank("acme", &lists, &chats);
        assert!(hits[0].top);
        assert_eq!(hits[0].kind, HitKind::Project);
        assert_eq!(hits.iter().filter(|h| h.top).count(), 1);
        assert_eq!(of_kind(&hits, HitKind::Project).len(), 1);
    }

    #[test]
    fn no_top_hit_from_a_single_group() {
        let lists = PaletteLists {
            routines: Some(vec![routine("Morning brief")]),
            ..PaletteLists::default()
        };
        let hits = rank("morning", &lists, &none());
        assert!(hits.iter().all(|h| !h.top));
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

    #[test]
    fn a_project_only_coding_agents_worked_in_is_found() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/relay", "build-box")]),
            sessions: Some(vec![
                agent_session("1", Some("Desktop UI"), "/work/widget", "build-box", 5),
                agent_session("2", None, "/work/widget", "mac-box", 9),
            ]),
            ..PaletteLists::default()
        };
        let hits = rank("widget", &lists, &none());
        let folders = of_kind(&hits, HitKind::Folder);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].id, "widget");
        assert_eq!(folders[0].title, "widget");
        assert_eq!(
            folders[0].detail,
            "Not imported · 2 sessions · build-box, mac-box"
        );
        assert!(of_kind(&hits, HitKind::Project).is_empty());
    }

    #[test]
    fn a_board_project_that_is_not_imported_opens_its_chat() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/relay", "build-box")]),
            boards: Some(vec![board("acme/relay", 1), board("acme/widget", 2)]),
            ..PaletteLists::default()
        };
        let hits = rank("acme", &lists, &none());
        let imported = of_kind(&hits, HitKind::Project);
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].id, "acme/relay");
        let chats = of_kind(&hits, HitKind::ProjectChat);
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].title, "acme/widget");
        assert_eq!(chats[0].id, Uuid::from_u128(2).to_string());
    }

    #[test]
    fn projects_are_found_once_across_lists() {
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
        let hits = rank("e", &lists, &none());
        let titles: Vec<&str> = hits
            .iter()
            .filter(|h| {
                matches!(
                    h.kind,
                    HitKind::Project | HitKind::ProjectChat | HitKind::Folder
                )
            })
            .map(|h| h.title.as_str())
            .collect();
        assert_eq!(titles, ["acme/relay", "acme/widget"]);
    }

    #[test]
    fn projects_from_every_list_rank_by_tier() {
        let lists = PaletteLists {
            projects: Some(vec![imported("acme/prewidget", "build-box")]),
            sessions: Some(vec![agent_session(
                "1",
                None,
                "/srv/widget",
                "build-box",
                1,
            )]),
            ..PaletteLists::default()
        };
        let hits = rank("widget", &lists, &none());
        let kinds: Vec<HitKind> = hits
            .iter()
            .filter(|h| matches!(h.kind, HitKind::Project | HitKind::Folder))
            .map(|h| h.kind)
            .collect();
        assert_eq!(kinds, [HitKind::Folder, HitKind::Project]);
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
        for query in ["desktop", "widget", "claude code", "build-box"] {
            let hits = rank(query, &lists, &none());
            let found = of_kind(&hits, HitKind::AgentSession);
            assert_eq!(found.len(), 1, "{query}");
            assert_eq!(found[0].title, "Cross-machine desktop UI");
            assert_eq!(found[0].detail, "Claude Code · widget · build-box");
            assert!(found[0].session.is_some());
        }
        let hits = rank("nothing-like-it", &lists, &none());
        assert!(of_kind(&hits, HitKind::AgentSession).is_empty());
    }

    #[test]
    fn coding_agents_list_best_match_then_newest() {
        let lists = PaletteLists {
            sessions: Some(vec![
                agent_session("old", Some("Widget login"), "/srv/a", "build-box", 1),
                agent_session("sub", Some("Fix widget"), "/srv/b", "build-box", 9),
                agent_session("new", Some("Widget sync"), "/srv/c", "build-box", 5),
            ]),
            ..PaletteLists::default()
        };
        let hits = rank("widget", &lists, &none());
        let titles: Vec<&str> = of_kind(&hits, HitKind::AgentSession)
            .iter()
            .map(|h| h.title.as_str())
            .collect();
        assert_eq!(titles, ["Widget sync", "Widget login", "Fix widget"]);
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
        let hits = rank("widget", &lists, &none());
        let found = of_kind(&hits, HitKind::AgentSession);
        assert_eq!(found[0].title, "acme/widget");
        assert_eq!(found[0].detail, "Claude Code · acme/widget · build-box");
        assert!(of_kind(&hits, HitKind::Folder).is_empty());
    }
}
