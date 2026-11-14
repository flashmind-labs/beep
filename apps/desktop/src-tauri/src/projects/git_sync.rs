//! Pulling and pushing the branch a project's checkout has out, on the machine that holds it.

use super::branches::placed;
use crate::{
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        files::{PeerShell, Place},
    },
};
use beep_projects::{
    checkout_sync::{Move, read_local_state, read_state, run_move},
    model::branch_name,
};
use beep_proto::node::events::GitRead;
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

/// Where a checkout's state comes from.
enum ReadFrom {
    /// The local refs, from the subscription when one is live: no network wait.
    LocalRefs,
    /// The checkout after a fetch, so the counts follow the remote.
    Fetched,
}

async fn row(fleet: &Fleet, place: &Place, from: ReadFrom) -> GitRow {
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
        let state = match from {
            ReadFrom::LocalRefs => {
                let shell = shell
                    .snapshot_git(&place.folder, GitRead::State)
                    .await
                    .map_err(|e| e.to_string())?;
                read_local_state(&shell, &place.folder).await
            }
            ReadFrom::Fetched => read_state(&shell, &place.folder).await,
        };

        state.map_err(|e| format!("{e:#}"))
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

/// The branch each online machine's checkout of `project` has out, from local refs or after
/// a `fetch`. Nothing for a project the mesh does not record.
#[tauri::command]
pub async fn project_git_state(
    fleet: Machines<'_>,
    project: String,
    fetch: bool,
) -> Result<Vec<GitRow>> {
    if !fleet
        .mesh()
        .is_some_and(|mesh| beep_fleet::has_project(&mesh, &project))
    {
        return Ok(Vec::new());
    }

    let places = placed(&fleet, &project)?;
    Ok(join_all(places.iter().map(|place| {
        let from = if fetch {
            ReadFrom::Fetched
        } else {
            ReadFrom::LocalRefs
        };
        row(&fleet, place, from)
    }))
    .await)
}

/// Pulls (fast-forward only) or pushes `branch` of `project`'s checkout on `machine_id`, then
/// reads the checkout again.
#[tauri::command]
pub async fn project_git_move(
    fleet: Machines<'_>,
    project: String,
    machine_id: String,
    branch: String,
    direction: Move,
) -> Result<GitRow> {
    let branch = branch_name(&branch)
        .ok_or_else(|| DesktopError::BadRequest("That is not a branch name.".into()))?;
    let machine: EndpointId = fleet.machine(&machine_id)?;
    let name = fleet.name_of(machine);
    let place = placed(&fleet, &project)?
        .into_iter()
        .find(|place| place.machine == machine)
        .ok_or_else(|| DesktopError::BadRequest(format!("{name} has no checkout of {project}.")))?;
    let shell = PeerShell::on(&fleet, machine).await?;
    run_move(&shell, &place.folder, branch, direction)
        .await
        .map_err(|why| DesktopError::BadRequest(format!("{name}: {why}")))?;
    Ok(row(&fleet, &place, ReadFrom::Fetched).await)
}
