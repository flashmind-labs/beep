//! What the palette searches. `follow` reads it for the app's whole life,
//! each source apart so a slow one holds up none of the rest, and rebuilds
//! the index on each answer. The lists are saved in the views file, so a new
//! launch finds them before the machines answer.

use super::{
    index::build_index,
    places::{Place, retired},
    prompts::PromptIndex,
};
use crate::{
    center::{
        home::mesh_imports,
        home_sessions::{AgentSessions, HOME_CHANGED},
        section,
        view_cache::{View, ViewCache, cache_view},
    },
    fleet::Fleet,
    projects::branches::read_branches as read_project_branches,
};
use beep_fleet::Session as FleetSession;
use beep_proto::{ImportedProject, Machine, MachineSession};
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
/// The branches are read again this often, and whenever the palette opens.
const READ_EVERY: Duration = Duration::from_secs(300);
/// The least time between two reads of the branches, however often it opens.
const READ_GAP: Duration = Duration::from_secs(10);
/// The least time between two reads of the mesh's projects and machines.
const LOCAL_GAP: Duration = Duration::from_secs(2);
/// The sessions are read when a session starts or ends, when the palette
/// opens, and this often besides.
const SESSIONS_EVERY: Duration = Duration::from_secs(600);
/// The least time between two reads of the sessions.
const SESSIONS_GAP: Duration = Duration::from_secs(5);
/// The most projects whose branches are read.
const BRANCH_PROJECTS: usize = 8;

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
    /// The machines' coding-agent sessions.
    pub(super) sessions: Option<Vec<MachineSession>>,
    pub(super) branches: Option<Vec<BranchRow>>,
}

impl PaletteLists {
    fn prune(&mut self) {
        if let Some(projects) = &mut self.projects {
            projects.retain(|p| !self.forgotten_projects.contains(&p.project));
        }
        if let Some(branches) = &mut self.branches {
            branches.retain(|b| !self.forgotten_projects.contains(&b.project));
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

struct Held {
    lists: PaletteLists,
    /// The index as JSON, as the windows read it.
    index: String,
}

/// The lists and their index.
pub(crate) struct PaletteCache {
    held: Mutex<Held>,
    read_lists: Notify,
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
                // Only a head start: the machines' lists replace it.
                log::info!("the saved palette lists do not parse: {e}");
                PaletteLists::default()
            }
            None => PaletteLists::default(),
        };
        lists.prune();
        let index = index_json(&lists)?;
        Ok(Self {
            held: Mutex::new(Held { lists, index }),
            read_lists: Notify::new(),
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

    /// A machine started or ended sessions: read them again, soon.
    pub(crate) fn sessions_changed(&self) {
        self.read_sessions.notify_one();
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
pub(crate) async fn follow(app: AppHandle) {
    for event in [HOME_CHANGED, "sessions-changed"] {
        let handle = app.clone();
        app.listen(event, move |_| {
            handle.state::<PaletteCache>().read_sessions.notify_one()
        });
    }

    // A phone searches the machines; it reads no transcripts to index.
    #[cfg(desktop)]
    let prompts = super::prompts::follow(&app);
    #[cfg(mobile)]
    let prompts = std::future::ready(());

    join!(
        follow_local(&app),
        follow_branches(&app),
        follow_sessions(&app),
        prompts
    );
}

async fn follow_branches(app: &AppHandle) {
    let cache = app.state::<PaletteCache>();

    loop {
        read_branches(app).await;
        sleep(READ_GAP).await;
        select! {
            () = sleep(READ_EVERY) => {}
            () = cache.read_lists.notified() => {}
        }
    }
}

async fn follow_local(app: &AppHandle) {
    let mut changes = app.state::<Arc<Fleet>>().changes();

    loop {
        read_local_lists(app).await;
        // Changes come in bursts: one read covers the rest of the burst.
        sleep(LOCAL_GAP).await;

        select! {
            changed = changes.changed() => {
                if !changed {
                    return;
                }
            }
            // Machine details that change with no mesh change.
            () = sleep(READ_EVERY) => {}
        }
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
    let machines = fleet.status().machines();
    app.state::<PaletteCache>().update(app, |lists| {
        lists.machines = Some(machines);
    });
}

async fn follow_sessions(app: &AppHandle) {
    let cache = app.state::<PaletteCache>();

    loop {
        read_sessions(app).await;
        sleep(SESSIONS_GAP).await;
        select! {
            () = sleep(SESSIONS_EVERY) => {}
            () = cache.read_sessions.notified() => {}
        }
    }
}

/// Every machine is asked.
async fn read_sessions(app: &AppHandle) {
    let agents = app.state::<Arc<AgentSessions>>();
    let views = app.state::<Arc<ViewCache>>();
    let listed = agents.list(app, &views).await;
    let Some(sessions) = section("agent sessions", listed) else {
        return;
    };
    app.state::<PaletteCache>()
        .update(app, |lists| lists.sessions = Some(sessions));
    app.state::<Arc<super::prompts::PromptIndex>>().changed();
}

/// Each project's are read on a machine.
async fn read_branches(app: &AppHandle) {
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
        let fleet = app.state::<Arc<Fleet>>().inner().clone();
        reads.spawn(async move {
            let read = read_project_branches(&fleet, project.clone()).await;
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
