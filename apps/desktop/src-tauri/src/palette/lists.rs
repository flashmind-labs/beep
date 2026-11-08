//! What the palette searches. `follow` reads it for the app's whole life,
//! each source apart so a slow one holds up none of the rest, and rebuilds
//! the index on each answer. The server's lists are saved in the views file,
//! so a new launch finds them before the server answers.

use super::{
    index::build_index,
    places::{Place, retired},
    prompts::PromptIndex,
};
use crate::{
    beep::{Beep, state::LinkState},
    center::{
        home::{mesh_imports, with_server},
        home_sessions::AgentSessions,
        section,
        view_cache::{View, ViewCache, cache_view},
    },
    error::DesktopError,
    fleet::Fleet,
    machines::list::{self as machine_list, Outside},
    projects::reviews::read_branches as read_project_branches,
};
use beep_fleet::Session as FleetSession;
use beep_proto::{
    Cron, GitItem, GitItemKind, ImportedProject, ListCrons, ListGitItems, ListImportedProjects,
    ListMemories, ListProjects, Machine, MachineSession, Memory, ProjectAgent, Session,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Listener, Manager, State, ipc::Response};
use tokio::{join, select, sync::Notify, task::JoinSet, time::sleep};

/// Tells the windows to read the index again.
const PALETTE_CHANGED: &str = "palette-changed";
/// The lists are read again this often, and whenever the palette opens.
const READ_EVERY: Duration = Duration::from_secs(300);
/// The least time between two reads of the lists, however often it opens.
const READ_GAP: Duration = Duration::from_secs(10);
/// The least time between two reads of the chats, while one is busy.
const CHATS_GAP: Duration = Duration::from_secs(2);
/// The most projects whose branches are read.
const BRANCH_PROJECTS: usize = 8;

/// The lists the index is built from. A list stays `None` until it was read
/// once; after that the last answer stays when a read fails.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct PaletteLists {
    pub(super) memories: Option<Vec<Memory>>,
    pub(super) routines: Option<Vec<Cron>>,
    pub(super) machines: Option<Vec<Machine>>,
    pub(super) projects: Option<Vec<ImportedProject>>,
    #[serde(default)]
    pub(super) retired_places: Vec<Place>,
    #[serde(default)]
    pub(super) forgotten_projects: HashSet<String>,
    #[serde(default)]
    pub(super) forgotten_chats: HashSet<String>,
    /// The projects board, which also holds projects never imported.
    pub(super) boards: Option<Vec<ProjectAgent>>,
    /// The machines' coding-agent sessions.
    pub(super) sessions: Option<Vec<MachineSession>>,
    pub(super) prs: Option<Vec<PrRow>>,
    pub(super) branches: Option<Vec<BranchRow>>,
    /// From the local copy of the chats, which has its own file.
    #[serde(skip)]
    pub(super) chats: Option<Vec<Session>>,
    /// Set while the app has no subscription: the index leaves out what is the assistant's.
    #[serde(skip)]
    pub(super) without_subscription: bool,
}

impl PaletteLists {
    fn prune(&mut self) {
        if let Some(projects) = &mut self.projects {
            projects.retain(|p| !self.forgotten_projects.contains(&p.project));
        }
        if let Some(boards) = &mut self.boards {
            for board in boards
                .iter()
                .filter(|b| self.forgotten_projects.contains(&b.name))
            {
                self.forgotten_chats.insert(board.chat_id.to_string());
            }
            boards.retain(|b| !self.forgotten_projects.contains(&b.name));
        }
        if let Some(branches) = &mut self.branches {
            branches.retain(|b| !self.forgotten_projects.contains(&b.project));
        }
        if let Some(prs) = &mut self.prs {
            prs.retain(|pr| !self.forgotten_projects.contains(&pr.repo));
        }
        if let Some(chats) = &mut self.chats {
            chats.retain(|chat| !self.forgotten_chats.contains(&chat.id));
        }
        if let Some(sessions) = &mut self.sessions {
            let projects = self.projects.as_deref().unwrap_or_default();
            sessions.retain(|s| !retired(s, projects, &self.retired_places));
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

/// An open pull request of the user's, as the palette finds it.
#[derive(Serialize, Deserialize)]
pub(crate) struct PrRow {
    pub(super) repo: String,
    pub(super) number: u64,
    pub(super) title: String,
    pub(super) url: String,
    /// Why it is the user's, such as "PR · asks for your review".
    pub(super) detail: String,
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

struct Held {
    lists: PaletteLists,
    /// The index as JSON, as the windows read it.
    index: String,
}

/// The lists and their index.
pub(crate) struct PaletteCache {
    held: Mutex<Held>,
    read_lists: Notify,
    read_chats: Notify,
    read_sessions: Notify,
}

fn index_json(lists: &PaletteLists) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&build_index(lists))?)
}

impl PaletteCache {
    /// Starts from the lists saved in `views`, if it holds them.
    pub(crate) fn load(views: &ViewCache) -> anyhow::Result<Self> {
        let mut lists: PaletteLists = match views.get(View::Palette).map(serde_json::from_value) {
            Some(Ok(lists)) => lists,
            Some(Err(e)) => {
                // Only a head start: the server's lists replace it.
                log::info!("the saved palette lists do not parse: {e}");
                PaletteLists::default()
            }
            None => PaletteLists::default(),
        };
        lists.without_subscription = !crate::beep::SERVER_ENABLED;
        lists.prune();
        let index = index_json(&lists)?;
        Ok(Self {
            held: Mutex::new(Held { lists, index }),
            read_lists: Notify::new(),
            read_chats: Notify::new(),
            read_sessions: Notify::new(),
        })
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn projects(&self) -> Vec<ImportedProject> {
        self.held().lists.projects.clone().unwrap_or_default()
    }

    pub(super) fn sessions(&self) -> Vec<MachineSession> {
        self.held().lists.sessions.clone().unwrap_or_default()
    }

    pub(crate) fn project_list(
        &self,
        app: &AppHandle,
        projects: Vec<ImportedProject>,
        removed: Vec<Place>,
    ) {
        let changed = {
            let held = self.held();
            let previous: std::collections::BTreeSet<_> = held
                .lists
                .projects
                .iter()
                .flatten()
                .map(Place::from)
                .collect();
            let current = projects.iter().map(Place::from).collect();
            previous != current || held.lists.retired_places != removed
        };
        self.update(app, |lists| {
            lists
                .forgotten_projects
                .extend(removed.iter().map(|p| p.project.clone()));
            for project in &projects {
                lists.forgotten_projects.remove(&project.project);
            }
            lists.projects = Some(projects);
            lists.retired_places = removed;
        });
        if changed {
            let _ = app.emit("palette-text-changed", ());
        }
    }

    pub(super) fn forget_project(&self, app: &AppHandle, project: &str, removed: Vec<Place>) {
        self.update(app, |lists| {
            lists.forgotten_projects.insert(project.to_owned());
            lists.retired_places = removed;
        });
    }

    /// The session view supplies metadata before the lists have caught up.
    /// An already listed session stays on the normal refresh schedule.
    pub(crate) fn found_session(&self, app: &AppHandle, session: MachineSession) {
        let known = {
            let held = self.held();
            held.lists
                .sessions
                .iter()
                .flatten()
                .any(|s| s.machine_id == session.machine_id && s.id == session.id)
                || retired(
                    &session,
                    held.lists.projects.as_deref().unwrap_or_default(),
                    &held.lists.retired_places,
                )
        };
        if !known {
            self.remember_session(app, session);
        }
    }

    pub(super) fn remember_session(&self, app: &AppHandle, session: MachineSession) {
        self.update(app, |lists| {
            let sessions = lists.sessions.get_or_insert_default();
            sessions.retain(|old| old.machine_id != session.machine_id || old.id != session.id);
            sessions.push(session);
        });
    }

    /// The chats in the local copy changed: read them for the index, soon.
    pub(crate) fn chats_changed(&self) {
        self.read_chats.notify_one();
    }

    /// Signing in or out changed what the server lists: read everything again, soon.
    pub(crate) fn link_changed(&self) {
        self.read_lists.notify_one();
        self.read_chats.notify_one();
    }

    /// Changes the lists and rebuilds the index. The windows hear of it only
    /// when the index came out different.
    fn update(&self, app: &AppHandle, change: impl FnOnce(&mut PaletteLists)) {
        let changed = {
            let mut held = self.held();
            change(&mut held.lists);
            held.lists.prune();
            cache_view(&app.state::<Arc<ViewCache>>(), View::Palette, &held.lists);
            match index_json(&held.lists) {
                Ok(index) if index != held.index => {
                    held.index = index;
                    true
                }
                Ok(_) => false,
                Err(e) => {
                    log::warn!("writing the palette's index: {e}");
                    false
                }
            }
        };
        if changed && let Err(e) = app.emit(PALETTE_CHANGED, ()) {
            log::warn!("telling the windows the palette changed: {e}");
        }
    }
}

/// The index as JSON.
#[tauri::command]
pub fn center_palette_index(cache: State<'_, PaletteCache>) -> Response {
    Response::new(cache.held().index.clone())
}

/// The palette opened: read the lists again, without anyone waiting for it.
#[tauri::command]
pub fn center_palette_refresh(cache: State<'_, PaletteCache>) {
    cache.read_lists.notify_one();
    cache.read_sessions.notify_one();
}

/// Keeps the index current for the app's life.
pub(crate) async fn follow(app: AppHandle, beep: Arc<Beep>) {
    for event in ["home-changed", "sessions-changed"] {
        let handle = app.clone();
        app.listen(event, move |_| {
            handle.state::<PaletteCache>().read_sessions.notify_one()
        });
    }
    join!(
        follow_local(&app),
        follow_lists(&app, &beep),
        follow_chats(&app, &beep),
        follow_sessions(&app, &beep),
        super::prompts::follow(&app)
    );
}

async fn follow_lists(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    loop {
        // With no subscription the machines, projects and branches come from the mesh.
        let without = beep.link_state() != LinkState::Linked;
        cache.update(app, |lists| lists.without_subscription = without);
        read_lists(app, beep).await;
        sleep(READ_GAP).await;
        select! {
            () = sleep(READ_EVERY) => {}
            () = cache.read_lists.notified() => {}
        }
    }
}

async fn follow_chats(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    loop {
        // Walks every chat's messages, so off the async workers.
        let reader = Arc::clone(beep);
        let copy = tauri::async_runtime::spawn_blocking(move || reader.chats().sessions()).await;
        let chats = match copy {
            // The chats are the assistant's: none without a subscription.
            Ok(_) if beep.link_state() != LinkState::Linked => Some(Vec::new()),
            Ok(Some(chats)) => Some(chats),
            Ok(None) if beep.link_state() == LinkState::Linked => {
                section("chats", beep.sessions().await)
            }
            Ok(None) => None,
            Err(e) => {
                log::warn!("reading the chats for the palette: {e}");
                None
            }
        };
        if chats.is_some() {
            cache.update(app, |lists| lists.chats = chats);
        }
        sleep(CHATS_GAP).await;
        cache.read_chats.notified().await;
    }
}

/// Reads every list, then saves them for the next launch.
async fn read_lists(app: &AppHandle, beep: &Arc<Beep>) {
    join!(
        async {
            read_server_lists(app, beep).await;
            // Branches are read for the projects just listed.
            read_branches(app, beep).await;
        },
        read_prs(app, beep),
    );
}

async fn follow_local(app: &AppHandle) {
    loop {
        read_local_lists(app).await;
        sleep(Duration::from_secs(2)).await;
    }
}

/// Projects come from replicated mesh state, without scanning a machine.
async fn read_local_lists(app: &AppHandle) {
    let session = app.state::<Arc<FleetSession>>();
    let fleet = app.state::<Arc<Fleet>>();
    // The mesh opens in the background. An opening mesh is not an empty
    // project list and must not erase what was saved.
    if fleet.mesh().is_none() {
        return;
    }
    let index = app.state::<Arc<PromptIndex>>();
    let _projects = index.project_lock().await;
    let projects = mesh_imports(&fleet, &session).await;
    let removed = match index.sync_projects(projects.clone()).await {
        Ok(removed) => removed,
        Err(e) => {
            log::warn!("updating indexed projects: {e:#}");
            return;
        }
    };
    app.state::<PaletteCache>()
        .project_list(app, projects, removed);
    let machines = machine_list::machines(app, &fleet, &app.state::<Outside>());
    app.state::<PaletteCache>().update(app, |lists| {
        lists.machines = Some(machines);
    });
}

async fn read_server_lists(app: &AppHandle, beep: &Beep) {
    if beep.link_state() != LinkState::Linked {
        return;
    }
    let (memories, routines, listed, boards) = join!(
        beep.call(ListMemories {}),
        beep.call(ListCrons {}),
        beep.call(ListImportedProjects {}),
        beep.call(ListProjects {}),
    );
    let memories = section("memories", memories);
    let routines = section("routines", routines);
    let imported = app
        .state::<PaletteCache>()
        .held()
        .lists
        .projects
        .clone()
        .unwrap_or_default();
    let projects = section("projects", with_server(imported, listed));
    let boards = section("the projects board", boards);
    app.state::<PaletteCache>().update(app, |lists| {
        lists.memories = memories.or(lists.memories.take());
        lists.routines = routines.or(lists.routines.take());
        lists.projects = projects.or(lists.projects.take());
        lists.boards = boards.or(lists.boards.take());
    });
}

/// The server may ask the git hosts.
async fn read_prs(app: &AppHandle, beep: &Beep) {
    if beep.link_state() != LinkState::Linked {
        return;
    }
    let Some(git) = section("pull requests", beep.call(ListGitItems {}).await) else {
        return;
    };
    let prs = git
        .items
        .into_iter()
        .filter(|item| item.kind == (GitItemKind::PullRequest {}))
        .map(PrRow::from)
        .collect();
    app.state::<PaletteCache>()
        .update(app, |lists| lists.prs = Some(prs));
}

async fn follow_sessions(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    loop {
        read_sessions(app, beep).await;
        sleep(Duration::from_secs(5)).await;
        select! {
            () = sleep(Duration::from_secs(30)) => {}
            () = cache.read_sessions.notified() => {}
        }
    }
}

/// The server asks every machine.
async fn read_sessions(app: &AppHandle, beep: &Arc<Beep>) {
    let agents = app.state::<Arc<AgentSessions>>();
    let views = app.state::<Arc<ViewCache>>();
    let listed = agents.list(app, beep, &views).await;
    let Some(sessions) = section("agent sessions", listed) else {
        return;
    };
    app.state::<PaletteCache>()
        .update(app, |lists| lists.sessions = Some(sessions));
    app.state::<Arc<super::prompts::PromptIndex>>().changed();
}

/// Each project's are read on a machine.
async fn read_branches(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    let projects: Vec<String> = {
        let held = cache.held();
        let mut seen = HashSet::new();
        held.lists
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
        let beep = Arc::clone(beep);
        let fleet = app.state::<Arc<Fleet>>().inner().clone();
        reads.spawn(async move {
            let read = read_project_branches(&beep, &fleet, project.clone())
                .await
                .and_then(|found| found.ok_or(DesktopError::NoServer));
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
    cache.update(app, |lists| lists.branches = Some(rows));
}

#[cfg(test)]
#[path = "../tests/palette_lists.rs"]
mod tests;
