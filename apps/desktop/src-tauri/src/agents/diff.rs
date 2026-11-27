//! The diff viewer: what changed in a coding agent session's checkout, read
//! by the node where the checkout is.

use crate::{
    agents::{
        file_reads::{Places, places_of},
        files_live::FilesLive,
    },
    error::Result,
    fleet::{
        Machines,
        agents::{self as agent_nodes, PlaceAsk},
        files as direct,
    },
    projects::diff_view::{DiffView, view},
};
use beep_proto::{
    SessionId,
    node::agent::{AgentCommand, DiffAsk, Worktree},
};
use tauri::{State, WebviewWindow};

/// What changed in the session folder `root` on `machine_id`, against its
/// default branch, uncommitted and untracked changes in; with `uncommitted`,
/// against HEAD (`@` to git, which no `origin/` copy shadows). `only` asks
/// for those files in full, past the caps.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri exposes each argument to the web view.
pub async fn work_diff(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    only: Vec<String>,
    lockfiles: bool,
    uncommitted: bool,
    context: Option<u32>,
) -> Result<DiffView> {
    let base = if uncommitted { "@" } else { "" }.to_owned();
    let ask = DiffAsk {
        root,
        base,
        head: String::new(),
        only,
        lockfiles,
        context: context.unwrap_or(3),
    };
    let diff = direct::work_diff(&fleet, fleet.machine(&machine_id)?, ask).await?;
    Ok(view(diff))
}

/// Where the sessions `asks` on `machine_id` work, each from its machine:
/// the checkout, whether it is a linked worktree, the branch.
#[tauri::command]
pub async fn agent_places(
    window: WebviewWindow,
    fleet: Machines<'_>,
    live: State<'_, FilesLive>,
    machine_id: String,
    asks: Vec<PlaceAsk>,
) -> Result<Places> {
    // Asking makes the window follow the folders its sessions work in: a change comes as a view.
    let machine = fleet.member(&machine_id);
    if let Some(machine) = machine {
        live.follow_places(window.label(), machine, &asks);
    }

    let places = places_of(&fleet, &machine_id, &asks).await?;
    if let Some(machine) = machine {
        live.placed(machine, &asks, &places);
    }

    Ok(places)
}

/// The slash commands and skills the running session `session` offers on
/// `machine_id`. Empty for a machine outside the mesh.
#[tauri::command]
pub async fn agent_commands(
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
) -> Result<Vec<AgentCommand>> {
    match fleet.member(&machine_id) {
        Some(machine) => agent_nodes::commands(&fleet, machine, &session).await,
        None => Ok(Vec::new()),
    }
}

/// The worktrees of the repository holding the session folder `folder` on
/// `machine_id`, each saying whether `session` made it. Empty for a machine
/// outside the mesh.
#[tauri::command]
pub async fn agent_worktrees(
    window: WebviewWindow,
    fleet: Machines<'_>,
    live: State<'_, FilesLive>,
    machine_id: String,
    folder: String,
    session: Option<SessionId>,
) -> Result<Vec<Worktree>> {
    match fleet.member(&machine_id) {
        Some(machine) => {
            // Asking makes the window follow the folder: a change comes as a view.
            live.follow_worktrees(window.label(), machine, &folder, session.as_ref());
            agent_nodes::worktrees(&fleet, machine, &folder, session.as_ref()).await
        }
        None => Ok(Vec::new()),
    }
}
