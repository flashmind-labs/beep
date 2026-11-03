//! The command center's Cmd+K search: ranks actions, settings, projects,
//! coding-agent sessions, chats, memories, routines and machines against
//! what was typed.

use crate::{
    agents::SessionRow, beep::Beep, center::section, commands::MemoryView, error::Result,
    home_sessions::AgentSessions, machines::MachineView, view_cache::ViewCache,
};
use beep_proto::{
    ApiProvider, Cron, GitItem, GitItemKind, ImportedProject, ListCrons, ListGitItems,
    ListImportedProjects, ListMachines, ListMemories, ListProjectBranches, ListProjects,
    MachineSession, ProjectAgent, Session, SessionMatch,
};
use serde::Serialize;
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};
use strum::IntoEnumIterator;
use tauri::{AppHandle, State};
use tokio::{join, task::JoinSet};

type Service<'a> = State<'a, Arc<Beep>>;

/// Most hits of one kind.
const PER_KIND: usize = 8;
/// Chats listed before anything is typed.
const RECENT_CHATS: usize = 5;
/// Chat text is searched from this many characters.
const MIN_TEXT_QUERY: usize = 2;

/// The lists the palette searches in memory. A list is `None` when the
/// server could not give it, which hides its group.
#[derive(Default)]
pub(crate) struct PaletteLists {
    memories: Option<Vec<MemoryView>>,
    routines: Option<Vec<Cron>>,
    machines: Option<Vec<MachineView>>,
    projects: Option<Vec<ImportedProject>>,
    /// The projects board, which also holds projects never imported.
    boards: Option<Vec<ProjectAgent>>,
    /// The machines' coding-agent sessions. Read after the rest, since the
    /// server asks every machine.
    sessions: Option<Vec<MachineSession>>,
    prs: Option<Vec<PrRow>>,
    /// Read after the rest, since each project's are read on a machine.
    branches: Option<Vec<BranchRow>>,
}

/// A branch with commits its project's default branch does not have.
pub(crate) struct BranchRow {
    project: String,
    name: String,
    ahead: u32,
    subject: String,
}

/// An open pull request of the user's, as the palette finds it.
pub(crate) struct PrRow {
    repo: String,
    number: u64,
    title: String,
    url: String,
    /// Why it is the user's, such as "PR · asks for your review".
    detail: String,
}

impl From<GitItem> for PrRow {
    fn from(item: GitItem) -> Self {
        let why = if item.review_requested {
            "asks for your review"
        } else if item.authored {
            "yours"
        } else {
            "assigned to you"
        };
        let draft = if item.draft { ", draft" } else { "" };
        Self {
            detail: format!("PR · {why}{draft} · {}", item.host),
            repo: item.repo,
            number: item.number,
            title: item.title,
            url: item.url,
        }
    }
}

/// The lists `center_palette` fetched last.
#[derive(Default)]
pub(crate) struct PaletteCache(Mutex<PaletteLists>);

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

/// A setting the palette finds, and the section of the sheet that holds it.
struct Setting {
    title: String,
    section: SettingsSection,
}

/// Every setting the palette finds.
fn settings() -> Vec<Setting> {
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
        .map(|(title, section)| Setting {
            title: title.to_owned(),
            section,
        })
        .chain(ApiProvider::all().map(|p| Setting {
            title: format!("{} API key", p.name()),
            section: Creds,
        }))
        .collect()
}

#[derive(Debug, Serialize)]
pub(crate) struct Hit {
    kind: HitKind,
    id: String,
    title: String,
    detail: String,
    /// The one best hit, listed first under Top hit instead of its group.
    top: bool,
    /// The session an `AgentSession` hit opens.
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<SessionRow>,
}

/// How well a text matches, best first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    Prefix,
    WordStart,
    Substring,
    /// Only found in a chat's messages.
    Text,
}

/// `query` must be lowercase.
fn tier(query: &str, text: &str) -> Option<Tier> {
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
fn best(mut matched: Vec<(Tier, Hit)>) -> Vec<(Tier, Hit)> {
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
    Hit {
        kind: HitKind::Chat,
        id: session.id.clone(),
        title: session.title.clone(),
        detail: snippet.to_owned(),
        top: false,
        session: None,
    }
}

fn action_hit(action: PaletteAction) -> Hit {
    Hit {
        kind: HitKind::Action,
        id: <&'static str>::from(action).to_owned(),
        title: action.title().to_owned(),
        detail: String::new(),
        top: false,
        session: None,
    }
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

/// The harness's name as the app shows it, such as "Claude Code".
fn harness_name(agent: &str) -> &str {
    match agent {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// The last part of a path or of `org/repo`; `None` for a root or nothing.
fn last_part(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\']).find(|part| !part.is_empty())
}

/// True when `cwd` is `folder` or inside it.
fn within(cwd: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches(['/', '\\']);
    !folder.is_empty()
        && cwd
            .strip_prefix(folder)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
}

/// The project a session runs in: the imported project whose folder on its
/// machine holds it (the deepest), else its folder's name.
fn session_project<'a>(
    session: &'a MachineSession,
    imported: &'a [ImportedProject],
) -> Option<&'a str> {
    imported
        .iter()
        .filter(|p| p.machine_id == session.machine_id && within(&session.cwd, &p.folder))
        .max_by_key(|p| p.folder.len())
        .map(|p| p.project.as_str())
        .or_else(|| last_part(&session.cwd))
}

/// A folder coding agents worked in that no project covers.
struct FolderRow<'a> {
    name: &'a str,
    sessions: usize,
    machines: Vec<&'a str>,
}

/// Projects by name, once each: imported ones, then the board's, then the
/// folders coding agents worked in. A folder named like a listed
/// project's repo is that project.
fn project_hits(
    query: &str,
    lists: &PaletteLists,
    imported: &[ImportedProject],
) -> Vec<(Tier, Hit)> {
    let mut hits = Vec::new();
    let mut names: HashSet<&str> = HashSet::new();
    let mut repos: HashSet<String> = HashSet::new();
    for p in imported {
        // A project is imported once per machine it is on; it lists once.
        if !names.insert(p.project.as_str()) {
            continue;
        }
        repos.insert(last_part(&p.project).unwrap_or(&p.project).to_lowercase());
        let Some(tier) = tier(query, &p.project) else {
            continue;
        };
        hits.push((
            tier,
            project_hit(
                HitKind::Project,
                p.project.clone(),
                &p.project,
                p.machine_name.clone(),
            ),
        ));
    }
    for b in lists.boards.iter().flatten() {
        if !names.insert(b.name.as_str()) {
            continue;
        }
        repos.insert(last_part(&b.name).unwrap_or(&b.name).to_lowercase());
        let Some(tier) = tier(query, &b.name) else {
            continue;
        };
        let detail = "Project chat · not imported".to_owned();
        hits.push((
            tier,
            project_hit(HitKind::ProjectChat, b.chat_id.to_string(), &b.name, detail),
        ));
    }
    let mut folders: Vec<FolderRow> = Vec::new();
    for s in lists.sessions.iter().flatten() {
        let Some(name) = session_project(s, imported) else {
            continue;
        };
        if names.contains(name) || repos.contains(&name.to_lowercase()) {
            continue;
        }
        let at = match folders.iter().position(|f| f.name == name) {
            Some(at) => at,
            None => {
                folders.push(FolderRow {
                    name,
                    sessions: 0,
                    machines: Vec::new(),
                });
                folders.len() - 1
            }
        };
        let folder = &mut folders[at];
        folder.sessions += 1;
        if !folder.machines.contains(&s.machine_name.as_str()) {
            folder.machines.push(&s.machine_name);
        }
    }
    for f in folders {
        let Some(tier) = tier(query, f.name) else {
            continue;
        };
        let count = if f.sessions == 1 {
            "1 session".to_owned()
        } else {
            format!("{} sessions", f.sessions)
        };
        let detail = format!("Not imported · {count} · {}", f.machines.join(", "));
        hits.push((
            tier,
            project_hit(HitKind::Folder, f.name.to_owned(), f.name, detail),
        ));
    }
    best(hits)
}

fn project_hit(kind: HitKind, id: String, name: &str, detail: String) -> Hit {
    Hit {
        kind,
        id,
        title: name.to_owned(),
        detail,
        top: false,
        session: None,
    }
}

/// A session that matched, and when it last moved.
struct FoundSession {
    tier: Tier,
    at: SystemTime,
    hit: Hit,
}

/// Coding-agent sessions found by title, project, harness or machine,
/// the best match first, then the one that moved last.
fn session_hits(
    query: &str,
    lists: &PaletteLists,
    imported: &[ImportedProject],
) -> Vec<(Tier, Hit)> {
    let mut found: Vec<FoundSession> = lists
        .sessions
        .iter()
        .flatten()
        .filter_map(|s| {
            let project = session_project(s, imported);
            let harness = harness_name(&s.agent);
            let title = s
                .title
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .or(project)
                .unwrap_or(harness);
            let tier = [
                Some(title),
                project,
                Some(harness),
                Some(s.agent.as_str()),
                Some(s.machine_name.as_str()),
            ]
            .into_iter()
            .flatten()
            .filter_map(|text| tier(query, text))
            .min()?;
            let detail = [Some(harness), project, Some(s.machine_name.as_str())]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            let hit = Hit {
                kind: HitKind::AgentSession,
                id: format!("{} {}", s.machine_id, s.id),
                title: title.to_owned(),
                detail,
                top: false,
                session: Some(SessionRow::from(s.clone())),
            };
            Some(FoundSession {
                tier,
                at: s.last_activity,
                hit,
            })
        })
        .collect();
    found.sort_by_key(|f| (f.tier, Reverse(f.at)));
    found
        .into_iter()
        .take(PER_KIND)
        .map(|f| (f.tier, f.hit))
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
        let hit = Hit {
            kind: HitKind::Memory,
            id: m.id().to_owned(),
            title: m.content().to_owned(),
            detail: m.category().to_owned(),
            top: false,
            session: None,
        };
        Some((tier(&query, m.content())?, hit))
    });
    let routines = lists.routines.iter().flatten().filter_map(|r| {
        let hit = Hit {
            kind: HitKind::Routine,
            id: r.id.clone(),
            title: r.label.clone(),
            detail: r.schedule.clone(),
            top: false,
            session: None,
        };
        Some((tier(&query, &r.label)?, hit))
    });
    let machines = lists.machines.iter().flatten().filter_map(|m| {
        let hit = Hit {
            kind: HitKind::Machine,
            id: m.id().to_owned(),
            title: m.name().to_owned(),
            detail: if m.online() { "online" } else { "offline" }.to_owned(),
            top: false,
            session: None,
        };
        Some((tier(&query, m.name())?, hit))
    });
    let settings = settings()
        .into_iter()
        .filter_map(|Setting { title, section }| {
            let tier = tier(&query, &title)?;
            let hit = Hit {
                kind: HitKind::Setting,
                id: <&'static str>::from(section).to_owned(),
                detail: format!("Settings · {}", section.title()),
                title,
                top: false,
                session: None,
            };
            Some((tier, hit))
        });
    let imported = lists.projects.as_deref().unwrap_or_default();
    let projects = project_hits(&query, lists, imported);
    let sessions = session_hits(&query, lists, imported);
    let code = lists.prs.iter().flatten().filter_map(|pr| {
        let title = format!("{} · PR #{} {}", pr.repo, pr.number, pr.title);
        let tier = tier(&query, &pr.title)
            .or_else(|| tier(&query, &pr.repo))
            .or_else(|| tier(&query, &format!("#{}", pr.number)))?;
        let hit = Hit {
            kind: HitKind::Code,
            id: pr.url.clone(),
            title,
            detail: pr.detail.clone(),
            top: false,
            session: None,
        };
        Some((tier, hit))
    });
    let branches = lists.branches.iter().flatten().filter_map(|b| {
        let tier = tier(&query, &b.name).or_else(|| tier(&query, &b.project))?;
        let hit = Hit {
            kind: HitKind::Branch,
            id: format!("{} {}", b.project, b.name),
            title: format!("{} · {}", b.project, b.name),
            detail: format!("Branch · {} ahead · {}", b.ahead, b.subject),
            top: false,
            session: None,
        };
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

/// Fetches what the palette searches, when it opens. A list the server
/// cannot give hides its group.
#[tauri::command]
pub async fn center_palette(beep: Service<'_>, cache: State<'_, PaletteCache>) -> Result<()> {
    let (memories, routines, machines, projects, boards, git) = join!(
        beep.call(ListMemories {}),
        beep.call(ListCrons {}),
        beep.call(ListMachines {}),
        beep.call(ListImportedProjects {}),
        beep.call(ListProjects {}),
        beep.call(ListGitItems {}),
    );
    let lists = PaletteLists {
        memories: section("memories", memories)
            .map(|all| all.into_iter().map(MemoryView::from).collect()),
        routines: section("routines", routines),
        machines: section("machines", machines)
            .map(|all| all.into_iter().map(MachineView::from).collect()),
        projects: section("projects", projects),
        boards: section("the projects board", boards),
        sessions: None,
        prs: section("pull requests", git).map(|git| {
            git.items
                .into_iter()
                .filter(|item| item.kind == (GitItemKind::PullRequest {}))
                .map(PrRow::from)
                .collect()
        }),
        branches: None,
    };
    let mut cached = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
    // The last sessions stay found until `center_palette_sessions` answers.
    let sessions = cached.sessions.take();
    *cached = PaletteLists { sessions, ..lists };
    Ok(())
}

/// Reads the machines' coding-agent sessions for the palette to find.
/// Called after `center_palette`, without holding up the first search:
/// the server asks every machine.
#[tauri::command]
pub async fn center_palette_sessions(
    app: AppHandle,
    beep: Service<'_>,
    views: State<'_, Arc<ViewCache>>,
    agents: State<'_, Arc<AgentSessions>>,
    cache: State<'_, PaletteCache>,
) -> Result<()> {
    let listed = agents.list(&app, beep.inner(), views.inner()).await;
    let Some(sessions) = section("agent sessions", listed) else {
        return Ok(());
    };
    cache
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .sessions = Some(sessions);
    Ok(())
}

/// The most projects whose branches the palette reads when it opens.
const BRANCH_PROJECTS: usize = 8;

/// Reads the branches with work of the projects `center_palette` listed,
/// for the palette to find. Called after it, without holding up the
/// first search.
#[tauri::command]
pub async fn center_palette_branches(
    beep: Service<'_>,
    cache: State<'_, PaletteCache>,
) -> Result<()> {
    let projects: Vec<String> = {
        let lists = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut seen = HashSet::new();
        lists
            .projects
            .iter()
            .flatten()
            .filter(|p| seen.insert(p.project.clone()))
            .map(|p| p.project.clone())
            .take(BRANCH_PROJECTS)
            .collect()
    };
    let mut reads = JoinSet::new();
    for project in projects {
        let beep = Arc::clone(beep.inner());
        reads.spawn(async move {
            let read = beep
                .call(ListProjectBranches {
                    project: project.clone(),
                })
                .await;
            (project, read)
        });
    }
    let mut rows = Vec::new();
    while let Some(done) = reads.join_next().await {
        let (project, read) = match done {
            Ok(done) => done,
            Err(e) => {
                log::warn!("reading branches for the palette: {e}");
                continue;
            }
        };
        let Some(found) = section("branches", read) else {
            continue;
        };
        rows.extend(
            found
                .branches
                .into_iter()
                .filter(|b| b.ahead > 0 && b.name != found.default_branch)
                .map(|b| BranchRow {
                    project: project.clone(),
                    name: b.name,
                    ahead: b.ahead,
                    subject: b.subject,
                }),
        );
    }
    rows.sort_by(|a, b| (&a.project, &a.name).cmp(&(&b.project, &b.name)));
    cache
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .branches = Some(rows);
    Ok(())
}

#[tauri::command]
pub async fn center_find(
    beep: Service<'_>,
    cache: State<'_, PaletteCache>,
    query: String,
) -> Result<Vec<Hit>> {
    let wants_text = query.trim().chars().count() >= MIN_TEXT_QUERY;
    let (titles, text) = join!(beep.sessions(), async {
        if wants_text {
            Some(beep.search(query.trim().to_owned()).await)
        } else {
            None
        }
    });
    let titles = section("chats", titles).unwrap_or_default();
    let text = text
        .and_then(|found| section("chat text", found))
        .unwrap_or_default();
    let lists = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(rank(&query, &lists, &ChatHits { titles, text }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{AgentStatus, Machine, Memory, MemorySource};
    use std::time::Duration;
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
