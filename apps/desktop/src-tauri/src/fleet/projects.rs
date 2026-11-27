//! Projects, sync and the git hosts, through the mesh. The work is in `beep_fleet`; this gives it
//! the machines of the mesh.

use crate::error::{DesktopError, Result};
use beep_fleet::{Desk, Session};
use beep_proto::ApiError;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// The session over the mesh, as a command takes it: one HTTP client and the last reads of the
/// projects, for the whole run of the app.
pub(crate) type FleetState<'a> = State<'a, Arc<Session>>;

/// A desk over the mesh, waiting for it to open.
pub(crate) async fn desk(session: &Arc<Session>) -> Result<Desk> {
    Ok(session.desk().await?)
}

/// The desk and the name of the mesh project `id` is the id of, `None` for an id the mesh does not
/// know. It waits for the mesh to open, and fails when there is none.
pub(crate) async fn mesh_project(
    session: &Arc<Session>,
    id: Uuid,
) -> Result<Option<(Desk, String)>> {
    let desk = desk(session).await?;
    Ok(desk.project_name(id).map(|name| (desk, name)))
}

/// The desk and the name of the mesh project `id` is the id of, which the mesh must know.
pub(crate) async fn named(session: &Arc<Session>, id: Uuid) -> Result<(Desk, String)> {
    mesh_project(session, id)
        .await?
        .ok_or(DesktopError::Api(ApiError::NotFound {}))
}
