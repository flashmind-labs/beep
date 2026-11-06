//! Projects, sync and the git hosts, through the mesh. The work is in `beep_fleet`; this gives it
//! the machines of the mesh and mirrors a change to the server when one is linked, so phones and
//! the project agent see it.

use super::Fleet;
use crate::{
    beep::Beep,
    error::{DesktopError, Result},
};
use async_trait::async_trait;
use beep_fleet::{Desk, FleetError, Peers, Session};
use beep_mesh::{Mesh, PeerClient};
use beep_proto::{ImportedProject, MirrorImport, RemoveProject, node::EnvReport};
use iroh::EndpointId;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// The session over the mesh, as a command takes it: one HTTP client and the last reads of the
/// projects, for the whole run of the app.
pub(crate) type FleetState<'a> = State<'a, Arc<Session>>;

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

    fn me(&self) -> EndpointId {
        self.id()
    }

    async fn client(&self, machine: EndpointId) -> std::result::Result<PeerClient, FleetError> {
        Fleet::client(self, machine).await.map_err(into_fleet)
    }

    fn last_env(&self, machine: EndpointId) -> Option<EnvReport> {
        self.status().last_env(machine)
    }
}

/// A desk over the mesh, waiting for it to open.
pub(crate) async fn desk(session: &Arc<Session>) -> Result<Desk> {
    Ok(session.desk().await?)
}

/// The desk, or `None` when the mesh is not there and a server is linked to answer instead; with
/// neither, the error.
pub(crate) async fn desk_or_server(session: &Arc<Session>, beep: &Beep) -> Result<Option<Desk>> {
    match desk(session).await {
        Ok(desk) => Ok(Some(desk)),
        Err(e) if beep.has_server() => {
            log::info!("no mesh, so the server answers: {e}");
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// The desk and the name of the mesh project `id` is the id of. `None` for an id the server made.
/// It waits for the mesh to open, and fails when there is none, so an id the mesh would know is
/// never taken for a server's while the mesh is still opening.
pub(crate) async fn mesh_project(
    session: &Arc<Session>,
    id: Uuid,
) -> Result<Option<(Desk, String)>> {
    let desk = desk(session).await?;
    Ok(desk.project_name(id).map(|name| (desk, name)))
}

/// Tells the server that `imported` landed, so phones and the project agent list it. The server
/// is not needed for the import, so a refusal or no answer is logged and the import stands.
pub(crate) async fn mirror_import(beep: &Beep, imported: &ImportedProject) {
    if !beep.has_server() {
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

/// Tells the server a project was removed. The caller hears if it did not: the project is gone
/// from the mesh all the same.
pub(crate) async fn mirror_remove(beep: &Beep, project: &str) -> Result<()> {
    if !beep.has_server() {
        return Ok(());
    }
    let request = RemoveProject {
        project: project.to_owned(),
    };
    beep.call(request).await
}
