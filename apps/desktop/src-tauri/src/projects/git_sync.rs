//! Pulling and pushing the branch a project's checkout has out, on the machine that holds it.

use super::branches::{Placed, placed};
use crate::{
    beep::Service,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        files::{PeerShell, Place},
    },
};
use beep_projects::{
    checkout_sync::{Move, read_state, run_move},
    model::branch_name,
};
use futures::future::join_all;
use iroh::EndpointId;
use serde::Serialize;

/// One checkout's branch and how far it is from its upstream.
#[derive(Debug, Serialize)]
pub struct GitRow {
    machine_id: String,
    /// `None` on a detached HEAD.
    branch: Option<String>,
    /// `None` before the first push.
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    /// Why the checkout could not be read.
    error: Option<String>,
}

async fn row(fleet: &Fleet, place: &Place) -> GitRow {
    let mut row = GitRow {
        machine_id: place.machine.to_string(),
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        error: None,
    };
    let read = async {
        let shell = PeerShell::on(fleet, place.machine)
            .await
            .map_err(|e| e.to_string())?;
        read_state(&shell, &place.folder)
            .await
            .map_err(|e| format!("{e:#}"))
    };
    match read.await {
        Ok(state) => {
            row.branch = state.branch;
            row.upstream = state.upstream;
            row.ahead = state.ahead;
            row.behind = state.behind;
        }
        Err(why) => row.error = Some(why),
    }
    row
}

/// The branch each online machine's checkout of `project` has out. Nothing for a project only
/// the server knows.
#[tauri::command]
pub async fn project_git_state(
    beep: Service<'_>,
    fleet: Machines<'_>,
    project: String,
) -> Result<Vec<GitRow>> {
    let Placed::Mesh(places) = placed(&beep, &fleet, &project).await? else {
        return Ok(Vec::new());
    };
    Ok(join_all(places.iter().map(|place| row(&fleet, place))).await)
}

/// Pulls (fast-forward only) or pushes `branch` of `project`'s checkout on `machine_id`, then
/// reads the checkout again.
#[tauri::command]
pub async fn project_git_move(
    beep: Service<'_>,
    fleet: Machines<'_>,
    project: String,
    machine_id: String,
    branch: String,
    direction: Move,
) -> Result<GitRow> {
    let branch = branch_name(&branch)
        .ok_or_else(|| DesktopError::BadRequest("That is not a branch name.".into()))?;
    let machine: EndpointId = fleet
        .member(&machine_id)
        .ok_or_else(|| DesktopError::BadRequest("That machine is not in the mesh.".into()))?;
    let name = fleet.name_of(machine);
    let place = match placed(&beep, &fleet, &project).await? {
        Placed::Mesh(places) => places.into_iter().find(|place| place.machine == machine),
        Placed::Server => None,
    }
    .ok_or_else(|| DesktopError::BadRequest(format!("{name} has no checkout of {project}.")))?;
    let shell = PeerShell::on(&fleet, machine).await?;
    run_move(&shell, &place.folder, branch, direction)
        .await
        .map_err(|why| DesktopError::BadRequest(format!("{name}: {why}")))?;
    Ok(row(&fleet, &place).await)
}
