//! Projects, sync and the git hosts, through the mesh. The work is in `beep_fleet`; this gives it
//! the machines of the mesh and mirrors a change to the server when one is linked, so phones and
//! the project agent see it.

use super::Fleet;
use crate::{
    beep::Beep,
    error::{DesktopError, Result},
};
use async_trait::async_trait;
use beep_fleet::{Desk, FleetError, Peers};
use beep_mesh::{Mesh, PeerClient};
use beep_proto::{ImportedProject, MirrorImport, RemoveProject};
use iroh::EndpointId;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// The fleet, as a command takes it.
pub(crate) type FleetState<'a> = State<'a, Arc<Fleet>>;

fn into_fleet(e: DesktopError) -> FleetError {
    match e {
        DesktopError::Offline(name) => FleetError::Offline(name),
        DesktopError::NoMesh(why) => FleetError::NoMesh(why),
        DesktopError::Mesh(e) => FleetError::Mesh(e),
        other => FleetError::Other(anyhow::anyhow!("{other}")),
    }
}

#[async_trait]
impl Peers for Fleet {
    async fn mesh(&self) -> std::result::Result<Mesh, FleetError> {
        self.ready().await.map_err(into_fleet)
    }

    async fn client(&self, machine: EndpointId) -> std::result::Result<PeerClient, FleetError> {
        Fleet::client(self, machine).await.map_err(into_fleet)
    }
}

/// A desk over the mesh, waiting for it to open.
pub(crate) async fn desk(fleet: &Arc<Fleet>) -> Result<Desk> {
    Ok(Desk::open(fleet.clone()).await?)
}

/// The desk and the name of the mesh project `id` is the id of. `None` for an id the server made,
/// or while the mesh is not open.
pub(crate) async fn mesh_project(fleet: &Arc<Fleet>, id: Uuid) -> Option<(Desk, String)> {
    let desk = desk(fleet).await.ok()?;
    let name = desk.project_name(id)?;
    Some((desk, name))
}

/// Tells the server that `imported` landed, so phones and the project agent list it. The server
/// is not needed for the import, so a refusal or no answer is logged and the import stands.
pub(crate) async fn mirror_import(beep: &Beep, imported: &ImportedProject) {
    if beep.server().is_none() {
        return;
    }
    let request = MirrorImport {
        project: imported.project.clone(),
        machine_id: imported.machine_id.clone(),
        folder: imported.folder.clone(),
        url: imported.url.clone(),
    };
    if let Err(e) = beep.call(request).await {
        log::info!("the server did not take an import: {e}");
    }
}

/// Tells the server a project was removed, as [`mirror_import`] does.
pub(crate) async fn mirror_remove(beep: &Beep, project: &str) {
    if beep.server().is_none() {
        return;
    }
    let request = RemoveProject {
        project: project.to_owned(),
    };
    if let Err(e) = beep.call(request).await {
        log::info!("the server did not take a project's removal: {e}");
    }
}
