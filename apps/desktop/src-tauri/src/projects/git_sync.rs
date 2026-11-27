//! Pulling and pushing the branch a project's checkout has out, on the machine that holds it.

use super::branches::placed;
use crate::{
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        files::{PeerShell, Place},
        projections::GitStateRow,
    },
};
use beep_projects::{
    branches::switch_branch,
    checkout_sync::{Move, read_local_state, read_state, run_move},
    model::branch_name,
};
use beep_proto::node::events::GitRead;
use futures::future::join_all;
use iroh::EndpointId;
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::time::timeout;

/// Event fired with a `GitChanged` when a checkout's Git branch/ahead-behind projection changes
/// in the background.
const GIT_CHANGED: &str = "project-git-changed";
/// Fast wait for a `fetch: true` call before returning cached local state while fetch completes.
const FETCH_FAST_WAIT: Duration = Duration::from_millis(1500);

/// What `project-git-changed` carries: the checkout's new state.
#[derive(Clone, Debug, Serialize)]
struct GitChanged {
    project: String,
    row: GitRow,
}

/// One checkout's branch and how far it is from its upstream.
#[derive(Clone, Debug, Serialize)]
pub struct GitRow {
    machine_id: String,
    #[serde(flatten)]
    state: GitStateRow,
}

impl GitRow {
    fn new(machine: EndpointId, state: GitStateRow) -> Self {
        Self {
            machine_id: machine.to_string(),
            state,
        }
    }
}

/// Where a checkout's state comes from.
enum ReadFrom {
    /// The local refs, from the subscription when one is live: no network wait.
    LocalRefs,
    /// The checkout after a fetch, so the counts follow the remote.
    Fetched,
}

async fn row(fleet: &Fleet, place: &Place, from: ReadFrom) -> GitStateRow {
    let mut row = GitStateRow {
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

    let limit = if matches!(from, ReadFrom::Fetched) {
        std::time::Duration::from_secs(15)
    } else {
        std::time::Duration::from_secs(5)
    };

    match tokio::time::timeout(limit, read).await {
        Ok(Ok(state)) => {
            row.branch = state.branch;
            row.upstream = state.upstream;
            row.ahead = state.ahead;
            row.behind = state.behind;
        }
        Ok(Err(why)) => row.error = Some(why),
        Err(_) => row.error = Some("did not answer in time".into()),
    }

    row
}

/// The branch each online machine's checkout of `project` has out, from local refs or after
/// a `fetch`. Nothing for a project the mesh does not record.
#[tauri::command]
pub async fn project_git_state(
    app: AppHandle,
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
    let fleet_arc = fleet.inner().clone();

    if !fetch {
        let rows = join_all(places.into_iter().map(|place| {
            let app = app.clone();
            let fleet_arc = fleet_arc.clone();
            let project = project.clone();
            async move {
                if place.machine == fleet_arc.id() {
                    let git_state = &fleet_arc.projections().git_state;
                    let key = (place.machine, place.folder.clone());
                    let ticket = git_state.begin(&key);
                    let read = row(&fleet_arc, &place, ReadFrom::LocalRefs).await;
                    git_state.put_begun(key, read.clone(), ticket);
                    return Some(GitRow::new(place.machine, read));
                }

                let machine = place.machine;
                let key = (machine, place.folder.clone());
                let notify = {
                    let fleet = fleet_arc.clone();
                    let key = key.clone();
                    let project = project.clone();
                    move || {
                        if let Some(cached) = fleet.projections().git_state.peek(&key) {
                            let row = GitRow::new(machine, cached);
                            let _ = app.emit(GIT_CHANGED, GitChanged { project, row });
                        }
                    }
                };
                let fleet_fetch = fleet_arc.clone();
                let got =
                    fleet_arc
                        .projections()
                        .git_state
                        .read_fast_with_fresh(
                            key,
                            |_| Duration::from_secs(10),
                            PartialEq::eq,
                            notify,
                            move || async move {
                                Ok(row(&fleet_fetch, &place, ReadFrom::LocalRefs).await)
                            },
                        )
                        .await;
                match got {
                    Some(Ok(cached)) => Some(GitRow::new(machine, cached)),
                    Some(Err(_)) | None => None,
                }
            }
        }))
        .await;
        return Ok(rows.into_iter().flatten().collect());
    }

    let rows = join_all(places.into_iter().map(|place| {
        let app = app.clone();
        let fleet_arc = fleet_arc.clone();
        let project = project.clone();
        async move {
            let key = (place.machine, place.folder.clone());
            let prior = fleet_arc.projections().git_state.peek(&key);
            let (tx, rx) = tokio::sync::oneshot::channel();
            let fleet_bg = fleet_arc.clone();
            let place_bg = place.clone();
            let key_bg = key.clone();
            let app_bg = app.clone();
            let project_bg = project.clone();

            let ticket = fleet_arc.projections().git_state.begin(&key);

            tauri::async_runtime::spawn(async move {
                let cached = row(&fleet_bg, &place_bg, ReadFrom::Fetched).await;
                let git_state = &fleet_bg.projections().git_state;
                let changed = git_state.peek(&key_bg).as_ref() != Some(&cached);
                // A read or write that began after this fetch and was stored first is newer.
                let stored = git_state.put_begun(key_bg, cached.clone(), ticket);
                let fetched = GitRow::new(place_bg.machine, cached.clone());
                let timed_out = tx.send(fetched).is_err();

                if stored && timed_out && changed {
                    let row = GitRow::new(place_bg.machine, cached);
                    let _ = app_bg.emit(
                        GIT_CHANGED,
                        GitChanged {
                            project: project_bg,
                            row,
                        },
                    );
                }
            });

            match timeout(FETCH_FAST_WAIT, rx).await {
                Ok(Ok(fetched)) => Some(fetched),
                Ok(Err(_)) | Err(_) => prior.map(|cached| GitRow::new(place.machine, cached)),
            }
        }
    }))
    .await;
    Ok(rows.into_iter().flatten().collect())
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
    let shell = timeout(Duration::from_secs(4), PeerShell::on(&fleet, machine))
        .await
        .map_err(|_| DesktopError::TimedOut(format!("{name} did not answer in time.")))??;
    run_move(&shell, &place.folder, branch, direction)
        .await
        .map_err(|why| DesktopError::BadRequest(format!("{name}: {why}")))?;
    let updated = row(&fleet, &place, ReadFrom::Fetched).await;
    fleet
        .projections()
        .git_state
        .put((place.machine, place.folder), updated.clone());
    Ok(GitRow::new(machine, updated))
}

/// Switches the project's own checkout on the machine shown in Git history.
#[tauri::command]
pub async fn project_git_switch(
    app: AppHandle,
    fleet: Machines<'_>,
    project: String,
    machine_id: String,
    root: String,
    current: Option<String>,
    branch: String,
) -> Result<GitRow> {
    let branch = branch_name(&branch)
        .ok_or_else(|| DesktopError::BadRequest("That is not a branch name.".into()))?;
    let machine = fleet.machine(&machine_id)?;
    let name = fleet.name_of(machine);
    let place = placed(&fleet, &project)?
        .into_iter()
        .find(|place| place.machine == machine)
        .ok_or_else(|| DesktopError::BadRequest(format!("{name} has no checkout of {project}.")))?;
    let shell = PeerShell::on(&fleet, machine).await?;
    switch_branch(&shell, &place.folder, &root, current.as_deref(), branch).await?;

    let cached = match read_local_state(&shell, &place.folder).await {
        Ok(state) => GitStateRow {
            branch: state.branch,
            upstream: state.upstream,
            ahead: state.ahead,
            behind: state.behind,
            error: None,
        },
        Err(error) => GitStateRow {
            branch: Some(branch.to_owned()),
            upstream: None,
            ahead: 0,
            behind: 0,
            error: Some(error.to_string()),
        },
    };
    let updated = GitRow::new(machine, cached.clone());
    fleet
        .projections()
        .git_state
        .put((machine, place.folder), cached);
    let _ = app.emit(
        GIT_CHANGED,
        GitChanged {
            project,
            row: updated.clone(),
        },
    );

    Ok(updated)
}
