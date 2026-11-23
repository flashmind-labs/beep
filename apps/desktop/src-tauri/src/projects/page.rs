//! A project's page: its machines, folders and sessions.

use crate::{
    agents::sessions::SessionRow,
    error::{DesktopError, Result},
    fleet::{
        Machines, files as direct,
        projects::{FleetState, desk},
        session_lists::{ListId, SessionLists},
    },
    machines::list::MachineLists,
};
use beep_proto::{ProjectMachine, SyncthingPlace};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

/// The sessions coding agents ran in `project`'s folders, newest first. `None` until every
/// machine with a checkout answered once; the list then comes as a `session-list` event.
#[tauri::command]
pub async fn project_sessions(
    lists: State<'_, SessionLists>,
    fleet: Machines<'_>,
    project: String,
) -> Result<Option<Vec<SessionRow>>> {
    let places = super::branches::placed(&fleet, &project)?;
    Ok(lists.want(ListId::Project(project), places).await)
}

/// The sessions coding agents ran in every project of the mesh on the
/// machines online, newest first: Home's Done today. `None` as for `project_sessions`.
#[tauri::command]
pub async fn projects_sessions(
    lists: State<'_, SessionLists>,
    fleet: Machines<'_>,
) -> Result<Option<Vec<SessionRow>>> {
    let Some(mesh) = fleet.mesh() else {
        return Ok(Some(Vec::new()));
    };
    let online = fleet.online();
    let places: Vec<direct::Place> = beep_fleet::project_names(&mesh)
        .iter()
        .flat_map(|project| beep_fleet::places(&mesh, project))
        .filter(|(machine, _)| online.contains(machine))
        .map(|(machine, folder)| direct::Place { machine, folder })
        .collect();

    Ok(lists.want(ListId::Home, places).await)
}

/// One of the user's machines as a project's page lists it.
#[derive(Debug, Serialize)]
pub struct ProjectMachineRow {
    #[serde(flatten)]
    machine: ProjectMachine,
    /// The mesh holds the project, so Add reaches this machine.
    mesh: bool,
}

impl From<ProjectMachine> for ProjectMachineRow {
    fn from(machine: ProjectMachine) -> Self {
        Self {
            machine,
            mesh: true,
        }
    }
}

/// Every machine of the user's for `project`'s page: its folder there, or
/// whether the machine is looking for one.
#[tauri::command]
pub async fn project_machines(
    fleet: FleetState<'_>,
    project: String,
) -> Result<Option<Vec<ProjectMachineRow>>> {
    let machines = desk(&fleet).await?.project_machines(&project).await?;
    Ok(Some(
        machines.into_iter().map(ProjectMachineRow::from).collect(),
    ))
}

/// Puts `project` on `machine_id` in `folder`: the machine makes the folder and joins the
/// project's sync, which brings the files and `.git`. Gives the folder as the machine spells it.
#[tauri::command]
pub async fn project_add_to_machine(
    fleet: FleetState<'_>,
    project: String,
    machine_id: String,
    folder: String,
) -> Result<String> {
    let added = desk(&fleet)
        .await?
        .add_to_machine(&project, &machine_id, &folder)
        .await?;
    Ok(added.folder)
}

/// Points `project` on `machine_id` at `folder`, empty or missing (the others fill it) or a
/// checkout of the same repository (it joins and merges). The old folder stays on disk as it is.
/// Gives the folder as the machine spells it.
#[tauri::command]
pub async fn project_change_folder(
    fleet: FleetState<'_>,
    project: String,
    machine_id: String,
    folder: String,
) -> Result<String> {
    let changed = desk(&fleet)
        .await?
        .change_folder(&project, &machine_id, &folder)
        .await?;
    Ok(changed.folder)
}

/// Takes `project` off `machine_id`: the machine stops syncing it and leaves its list, and the
/// folder there stays as it is, or with `delete_folder` is deleted once the node there stopped
/// syncing it; then says what became of it. Refused for the last machine, and while an agent
/// session is open in the project there, so no agent is left working in a folder that no longer
/// syncs.
#[tauri::command]
pub async fn project_remove_from_machine(
    app: AppHandle,
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    held: State<'_, MachineLists>,
    project: String,
    machine_id: String,
    delete_folder: Option<bool>,
) -> Result<Option<String>> {
    let id = machines
        .member(&machine_id)
        .ok_or_else(|| DesktopError::BadRequest("That machine is not known.".into()))?;
    let folders: Vec<String> = machines
        .mesh()
        .map(|mesh| beep_fleet::places(&mesh, &project))
        .unwrap_or_default()
        .into_iter()
        .filter(|(machine, _)| *machine == id)
        .map(|(_, folder)| folder)
        .collect();
    let open = direct::has_open_project_session(&machines, id, &folders, &held).await?;
    if open {
        return Err(DesktopError::BadRequest(format!(
            "An agent is working in {project} on {}. Stop it first.",
            machines.name_of(id)
        )));
    }

    let cache = app.state::<crate::palette::PaletteCache>();
    let _projects = cache.project_lock().await;
    let desk = desk(&fleet).await?;
    let removed = desk
        .remove_from_machine(&project, &machine_id, delete_folder.unwrap_or(false))
        .await?;
    cache.project_list(&app, desk.imported().await);
    Ok(removed)
}

/// The machines on which Syncthing keeps `project`'s folder in step.
#[tauri::command]
pub async fn project_syncthing(
    fleet: FleetState<'_>,
    project: String,
) -> Result<Vec<SyncthingPlace>> {
    Ok(desk(&fleet).await?.syncthing(&project).await)
}
