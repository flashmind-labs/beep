//! What the palette searches. `follow` reads it for the app's whole life,
//! each source apart so a slow one holds up none of the rest, and rebuilds
//! the index on each answer. The lists are saved in the views file, so a new
//! launch finds them before the machines answer. The sessions are not kept
//! here: the index takes them from the session mirror each time it is built,
//! which is when a window asks for it or opens the palette, and adds the few
//! the user opened that the mirror does not hold.

use super::{
    index::build_index,
    places::{Place, retired},
};
use crate::{
    agents::sessions::overlaid,
    center::{
        home::mesh_imports,
        lock, section,
        view_cache::{View, ViewCache, cache_view},
    },
    fleet::{Fleet, session_mirror::SessionMirror},
    projects::branches::read_branches as read_project_branches,
};
use beep_fleet::Session as FleetSession;
use beep_proto::{ImportedProject, Machine, MachineSession};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State, ipc::Response};
use tokio::{join, sync::Notify, task::JoinSet, time::sleep};
use tokio_util::task::AbortOnDropHandle;

/// Tells the windows the index changed, with the index as JSON text.
const PALETTE_CHANGED: &str = "palette-changed";
/// The least time between two reads of the branches, however often it opens.
const READ_GAP: Duration = Duration::from_secs(10);
/// The least time between two reads of the mesh's projects and machines.
const LOCAL_GAP: Duration = Duration::from_secs(2);
/// The most projects whose branches are read.
const BRANCH_PROJECTS: usize = 8;
/// The most sessions kept that the user opened and the mirror does not hold.
const OPENED_MOST: usize = 50;

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
    fn set_projects(&mut self, projects: Vec<ImportedProject>) {
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
    fn read_projects(&mut self, read: u64, projects: Vec<ImportedProject>) -> bool {
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

    fn forget_project(&mut self, project: &str) {
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
    fn without_retired(&self, sessions: &mut Vec<MachineSession>) {
        let projects = self.projects.as_deref().unwrap_or_default();
        sessions.retain(|s| !retired(s, projects, &self.retired_places));
    }

    /// Keeps `session`, which the user opened, unless its place left.
    fn open(&mut self, session: MachineSession) {
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

    fn prune(&mut self) {
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

struct Held {
    lists: PaletteLists,
    /// The index as JSON, as the windows read it.
    index: String,
}

/// The lists and their index.
pub(crate) struct PaletteCache {
    held: Mutex<Held>,
    read_lists: Notify,
    /// Waits for the rows the opening of the palette asked for, which end with the palette's next
    /// build of the index.
    settling: Mutex<Option<AbortOnDropHandle<()>>>,
    /// Counts the reads of the mesh's projects and the removals, so a read
    /// taken before a removal cannot bring the project back.
    tickets: AtomicU64,
}

fn index_json(lists: &PaletteLists, sessions: &[MachineSession]) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&build_index(lists, sessions))?)
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
        // The sessions join the index once the mirror holds them.
        let index = index_json(&lists, &[])?;
        Ok(Self {
            held: Mutex::new(Held { lists, index }),
            read_lists: Notify::new(),
            settling: Mutex::new(None),
            tickets: AtomicU64::new(0),
        })
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        lock(&self.held)
    }

    pub(crate) fn projects(&self) -> Vec<ImportedProject> {
        self.held().lists.projects.clone().unwrap_or_default()
    }

    /// The mirror's sessions as the windows list them, with the opened ones it lacks, less
    /// those of removed places.
    pub(super) fn sessions(&self, app: &AppHandle) -> Vec<MachineSession> {
        let fleet = app.state::<Arc<Fleet>>();
        let rows = app.state::<Arc<SessionMirror>>().current();
        let mut sessions = overlaid(app, &fleet, &rows);
        let held = self.held();
        let lacking: Vec<_> = held
            .lists
            .opened
            .iter()
            .filter(|opened| {
                !sessions
                    .iter()
                    .any(|s| s.machine_id == opened.machine_id && s.id == opened.id)
            })
            .cloned()
            .collect();
        sessions.extend(lacking);
        held.lists.without_retired(&mut sessions);

        sessions
    }

    /// The user opened `session`: the palette finds it even when the mirror does not hold it.
    pub(crate) fn found_session(&self, app: &AppHandle, session: MachineSession) {
        let held = self
            .sessions(app)
            .iter()
            .any(|s| s.machine_id == session.machine_id && s.id == session.id);

        if !held {
            self.update(app, |lists| lists.open(session));
        }
    }

    pub(super) fn machine_names(&self) -> HashMap<String, String> {
        self.held()
            .lists
            .machines
            .iter()
            .flatten()
            .map(|machine| (machine.id.clone(), machine.name.clone()))
            .collect()
    }

    /// The ticket of a read of the mesh's projects that starts now.
    pub(crate) fn ticket(&self) -> u64 {
        self.tickets.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// `projects` are the imports as the mesh held them at the read with ticket `read`.
    pub(crate) fn project_list(&self, app: &AppHandle, read: u64, projects: Vec<ImportedProject>) {
        let mut changed = false;
        self.update(app, |lists| changed = lists.read_projects(read, projects));

        if changed {
            let _ = app.emit("palette-text-changed", ());
        }
    }

    /// A machine's place of a project was removed: a read taken before now may still hold it.
    pub(crate) fn place_removed(&self, app: &AppHandle) {
        let removed_at = self.ticket();
        self.update(app, |lists| lists.removed_at = removed_at);
    }

    pub(crate) fn forget_project(&self, app: &AppHandle, project: &str) {
        let removed_at = self.ticket();
        self.update(app, |lists| {
            lists.removed_at = removed_at;
            lists.forget_project(project);
        });
        let _ = app.emit("palette-text-changed", ());
    }

    /// Changes the lists and rebuilds the index. The windows hear of it only
    /// when the index came out different.
    fn update(&self, app: &AppHandle, change: impl FnOnce(&mut PaletteLists)) {
        {
            let mut held = self.held();
            change(&mut held.lists);
            held.lists.prune();
            cache_view(&app.state::<Arc<ViewCache>>(), View::Palette, &held.lists);
        }

        // The sessions follow the lists as changed: opened ones and removed places count.
        let sessions = self.sessions(app);
        let changed = {
            let mut held = self.held();

            match index_json(&held.lists, &sessions) {
                Ok(index) if index != held.index => {
                    held.index = index.clone();
                    Some(index)
                }
                Ok(_) => None,
                Err(e) => {
                    log::warn!("writing the palette's index: {e}");
                    None
                }
            }
        };

        if let Some(index) = changed
            && let Err(e) = app.emit(PALETTE_CHANGED, index)
        {
            log::warn!("telling the windows the palette changed: {e}");
        }
    }
}

/// The index as JSON, with the sessions the mirror holds now.
#[tauri::command]
pub fn center_palette_index(app: AppHandle, cache: State<'_, PaletteCache>) -> Response {
    cache.update(&app, |_| {});

    Response::new(cache.held().index.clone())
}

/// The palette opened: read the lists again, without anyone waiting for it. The windows get
/// the index with the sessions now held, and again once the rows this asks for arrive.
#[tauri::command]
pub async fn center_palette_refresh(app: AppHandle) {
    let cache = app.state::<PaletteCache>();
    let mirror = app.state::<Arc<SessionMirror>>();
    let mut rows = mirror.watch();
    cache.read_lists.notify_one();
    // Lists what is old; the mirror's rows follow into the index.
    mirror.rows();
    cache.update(&app, |_| {});

    let handle = app.clone();
    let settling = tokio::spawn(async move {
        if rows.changed().await.is_ok() {
            handle.state::<PaletteCache>().update(&handle, |_| {});
        }
    });
    *lock(&cache.settling) = Some(AbortOnDropHandle::new(settling));
}

/// Keeps the index current for the app's life.
pub(crate) async fn follow(app: AppHandle) {
    // A phone searches the machines' indexes; a desktop keeps copies.
    #[cfg(desktop)]
    let copies = super::copies::follow(app.clone());
    #[cfg(mobile)]
    let copies = std::future::ready(());

    join!(follow_local(&app), follow_branches(&app), copies);
}

async fn follow_branches(app: &AppHandle) {
    let cache = app.state::<PaletteCache>();

    loop {
        read_branches(app).await;
        sleep(READ_GAP).await;
        cache.read_lists.notified().await;
    }
}

async fn follow_local(app: &AppHandle) {
    let mut changes = app.state::<Arc<Fleet>>().changes();

    loop {
        read_local_lists(app).await;
        // Changes come in bursts: one read covers the rest of the burst.
        sleep(LOCAL_GAP).await;

        if !changes.changed().await {
            return;
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

    let cache = app.state::<PaletteCache>();
    let read = cache.ticket();
    let projects = mesh_imports(&fleet, &session).await;
    cache.project_list(app, read, projects);
    let machines = fleet.status().machines();
    app.state::<PaletteCache>().update(app, |lists| {
        lists.machines = Some(machines);
    });
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
