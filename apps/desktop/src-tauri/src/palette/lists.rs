//! What the palette searches. `follow` reads it for the app's whole life,
//! each source apart so a slow one holds up none of the rest, and rebuilds
//! the index on each change of the lists, which `palette_store` keeps and saves. The sessions are not kept
//! here: the index takes them from the session mirror each time it is built,
//! which is when a window asks for it or opens the palette, and adds the few
//! the user opened that the mirror does not hold.

use super::index::build_index;
use crate::{
    agents::overlay::overlaid,
    center::{section, sections::mesh_imports},
    fleet::{Fleet, session_mirror::SessionMirror},
    palette_store::{
        PaletteStore,
        lists::{BranchRow, PaletteLists},
    },
    projects::branches::read_branches as read_project_branches,
    store::lock,
};
use beep_fleet::Session as FleetSession;
use beep_proto::MachineSession;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
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

/// The index, as the windows read it.
pub(crate) struct PaletteCache {
    index: Mutex<String>,
    read_lists: Notify,
    /// Waits for the rows the opening of the palette asked for, which end with the palette's next
    /// build of the index.
    settling: Mutex<Option<AbortOnDropHandle<()>>>,
}

fn index_json(lists: &PaletteLists, sessions: &[MachineSession]) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&build_index(lists, sessions))?)
}

impl PaletteCache {
    /// Starts from the lists `store` holds.
    pub(crate) fn load(store: &PaletteStore) -> anyhow::Result<Self> {
        // The sessions join the index once the mirror holds them.
        let index = store.read(|lists| index_json(lists, &[]))?;

        Ok(Self {
            index: Mutex::new(index),
            read_lists: Notify::new(),
            settling: Mutex::new(None),
        })
    }

    /// The mirror's sessions as the windows list them, with the opened ones it lacks, less
    /// those of removed places.
    pub(super) fn sessions(&self, app: &AppHandle) -> Vec<MachineSession> {
        let fleet = app.state::<Arc<Fleet>>();
        let rows = app.state::<Arc<SessionMirror>>().current();
        let mut sessions = overlaid(app, &fleet, &rows);
        app.state::<PaletteStore>().read(|lists| {
            let lacking: Vec<_> = lists
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
            lists.without_retired(&mut sessions);
        });

        sessions
    }

    pub(super) fn machine_names(&self, app: &AppHandle) -> HashMap<String, String> {
        app.state::<PaletteStore>().read(|lists| {
            lists
                .machines
                .iter()
                .flatten()
                .map(|machine| (machine.id.clone(), machine.name.clone()))
                .collect()
        })
    }

    /// Builds the index again. The windows hear of it only when it came out different.
    fn rebuild(&self, app: &AppHandle) {
        // The sessions follow the lists as changed: opened ones and removed places count.
        let sessions = self.sessions(app);
        let changed = {
            let mut held = lock(&self.index);
            let built = app
                .state::<PaletteStore>()
                .read(|lists| index_json(lists, &sessions));

            match built {
                Ok(index) if index != *held => {
                    *held = index.clone();
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
    cache.rebuild(&app);

    Response::new(lock(&cache.index).clone())
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
    cache.rebuild(&app);

    let handle = app.clone();
    let settling = tokio::spawn(async move {
        if rows.changed().await.is_ok() {
            handle.state::<PaletteCache>().rebuild(&handle);
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

    join!(
        follow_store(&app),
        follow_local(&app),
        follow_branches(&app),
        copies
    );
}

/// Builds the index again on each change of the lists.
async fn follow_store(app: &AppHandle) {
    let mut changes = app.state::<PaletteStore>().watch();

    loop {
        app.state::<PaletteCache>().rebuild(app);

        if changes.changed().await.is_err() {
            return;
        }
    }
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

    let store = app.state::<PaletteStore>();
    let read = store.ticket();
    let projects = mesh_imports(&fleet, &session).await;
    store.project_list(app, read, projects);
    let machines = fleet.status().machines();
    store.update(app, |lists| {
        lists.machines = Some(machines);
    });
}

/// Each project's are read on a machine.
async fn read_branches(app: &AppHandle) {
    let store = app.state::<PaletteStore>();
    let projects: Vec<String> = store.read(|lists| {
        let mut seen = HashSet::new();
        lists
            .projects
            .iter()
            .flatten()
            .filter(|p| seen.insert(p.project.clone()))
            .map(|p| p.project.clone())
            .take(BRANCH_PROJECTS)
            .collect()
    });
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
    store.update(app, |lists| lists.branches = Some(rows));
}

#[cfg(test)]
#[path = "../tests/palette_lists.rs"]
mod tests;
