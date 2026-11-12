//! A project's page: its machines, folders and sessions.

use crate::{
    agents::sessions::SessionRow,
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::{
        Machines, files as direct,
        projects::{FleetState, desk},
    },
};
use beep_fleet::project_id;
use beep_proto::{AgentStatus, ProjectAgent, ProjectMachine, SyncthingPlace};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// The sessions coding agents ran in `project`'s folders, newest first.
#[tauri::command]
pub async fn project_sessions(fleet: Machines<'_>, project: String) -> Result<Vec<SessionRow>> {
    let places = super::branches::placed(&fleet, &project)?;
    let mut sessions = direct::project_sessions(&fleet, &places).await;

    if let Some(mesh) = fleet.mesh() {
        for session in &mut sessions {
            beep_fleet::session_titles::apply(&mesh, session);
        }
    }

    Ok(sessions.into_iter().map(SessionRow::from).collect())
}

/// The sessions coding agents ran in every project of the mesh on the
/// machines online, newest first, each machine asked once: Home's Done today.
#[tauri::command]
pub async fn projects_sessions(fleet: Machines<'_>) -> Result<Vec<SessionRow>> {
    let Some(mesh) = fleet.mesh() else {
        return Ok(Vec::new());
    };
    let online = fleet.online();
    let places: Vec<direct::Place> = beep_fleet::project_names(&mesh)
        .iter()
        .flat_map(|project| beep_fleet::places(&mesh, project))
        .filter(|(machine, _)| online.contains(machine))
        .map(|(machine, folder)| direct::Place { machine, folder })
        .collect();
    let mut sessions = direct::project_sessions(&fleet, &places).await;

    for session in &mut sessions {
        beep_fleet::session_titles::apply(&mesh, session);
    }

    Ok(sessions.into_iter().map(SessionRow::from).collect())
}

/// One of the user's machines as a project's page lists it.
#[derive(Debug, Serialize)]
pub struct ProjectMachineRow {
    id: String,
    name: String,
    online: bool,
    /// The project's folder there; `None` without a checkout.
    folder: Option<String>,
    /// The machine is looking through it for a checkout now.
    checking: bool,
    /// In milliseconds since the Unix epoch.
    checked_at: Option<u64>,
    /// The mesh holds the project, so Add reaches this machine.
    mesh: bool,
}

impl From<ProjectMachine> for ProjectMachineRow {
    fn from(m: ProjectMachine) -> Self {
        Self {
            id: m.machine_id,
            name: m.machine_name,
            online: m.online,
            folder: m.folder,
            checking: m.checking,
            checked_at: m.checked_at.map(epoch_ms),
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
    project: String,
    machine_id: String,
    delete_folder: Option<bool>,
) -> Result<Option<String>> {
    let id = machines
        .member(&machine_id)
        .ok_or_else(|| DesktopError::BadRequest("That machine is not known.".into()))?;
    let places: Vec<direct::Place> = machines
        .mesh()
        .map(|mesh| beep_fleet::places(&mesh, &project))
        .unwrap_or_default()
        .into_iter()
        .filter(|(machine, _)| *machine == id)
        .map(|(machine, folder)| direct::Place { machine, folder })
        .collect();
    let open = direct::project_sessions(&machines, &places)
        .await
        .into_iter()
        .any(|session| !matches!(session.status, AgentStatus::Ended {}));
    if open {
        return Err(DesktopError::BadRequest(format!(
            "An agent is working in {project} on {}. Stop it first.",
            machines.name_of(id)
        )));
    }

    let index = app.state::<Arc<crate::palette::prompts::PromptIndex>>();
    let _projects = index.project_lock().await;
    let desk = desk(&fleet).await?;
    let removed = desk
        .remove_from_machine(&project, &machine_id, delete_folder.unwrap_or(false))
        .await?;
    let projects = desk.imported().await;
    let retired = index.sync_projects(projects.clone()).await?;
    app.state::<crate::palette::PaletteCache>()
        .project_list(&app, projects, retired);
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

/// The page of the mesh project `name` (`org/repo`), on the id its name
/// makes. There is no project agent, so it has no chat.
#[tauri::command]
pub async fn center_open_project(fleet: FleetState<'_>, name: String) -> Result<ProjectAgent> {
    desk(&fleet).await?.project_machines(&name).await?;
    Ok(ProjectAgent {
        project_id: project_id(&name),
        name,
        chat_id: Uuid::nil(),
        open_tasks: 0,
        open_workers: 0,
    })
}
