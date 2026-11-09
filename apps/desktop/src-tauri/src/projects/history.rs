//! History of an imported project, read over its existing mesh shell.

use super::branches::{Placed, placed};
use crate::{
    agents::diff::{DiffView, view},
    beep::Service,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        files::{PeerShell, Place},
    },
};
use beep_projects::history::{self, ChangedFile, CommitId, History, HistoryAsk};
use beep_proto::node::{agent::DiffAsk, events::GitRead};
use serde::Serialize;
use std::str::FromStr;

#[derive(Serialize)]
pub struct HistoryView {
    machine_id: String,
    machine_name: String,
    #[serde(flatten)]
    history: History,
}

async fn location(
    beep: &crate::beep::Beep,
    fleet: &Fleet,
    project: &str,
    machine: Option<&str>,
) -> Result<Place> {
    let Placed::Mesh(places) = placed(beep, fleet, project).await? else {
        return Err(DesktopError::BadRequest(
            "Git history needs an online mesh checkout of this project.".to_owned(),
        ));
    };
    places
        .into_iter()
        .find(|p| machine.is_none_or(|id| p.machine.to_string() == id))
        .ok_or_else(|| {
            DesktopError::BadRequest("The checkout used for this history is offline.".to_owned())
        })
}

#[tauri::command]
pub async fn project_git_history(
    beep: Service<'_>,
    fleet: Machines<'_>,
    project: String,
    machine_id: Option<String>,
    revision: Option<String>,
    offset: u32,
    search: String,
) -> Result<HistoryView> {
    let place = location(&beep, &fleet, &project, machine_id.as_deref()).await?;
    let shell = PeerShell::on(&fleet, place.machine)
        .await?
        .snapshot_git(
            &place.folder,
            GitRead::History {
                revision: revision.clone(),
                offset,
                search: search.clone(),
            },
        )
        .await?
        .complete_output();
    let revision = revision.as_deref().map(CommitId::from_str).transpose()?;
    let ask = HistoryAsk {
        folder: &place.folder,
        revision: revision.as_ref(),
        offset,
        search: &search,
    };
    let history = history::parse_history(
        history::run(&shell, &history::history_script(&ask)).await?,
        offset,
    )?;
    Ok(HistoryView {
        machine_id: place.machine.to_string(),
        machine_name: fleet.name_of(place.machine),
        history,
    })
}

#[tauri::command]
pub async fn project_git_files(
    beep: Service<'_>,
    fleet: Machines<'_>,
    project: String,
    machine_id: String,
    commit: String,
) -> Result<Vec<ChangedFile>> {
    let place = location(&beep, &fleet, &project, Some(&machine_id)).await?;
    let shell = PeerShell::on(&fleet, place.machine)
        .await?
        .snapshot_git(
            &place.folder,
            GitRead::Files {
                commit: commit.clone(),
            },
        )
        .await?
        .complete_output();
    let id = CommitId::from_str(&commit)?;
    let mut parts = history::run(&shell, &history::files_script(&place.folder, &id)).await?;
    let files = parts.remove("files").ok_or_else(|| {
        DesktopError::BadRequest("Git did not list the changed files.".to_owned())
    })?;
    Ok(history::parse_files(&files)?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri exposes each argument to the web view.
pub async fn project_git_diff(
    beep: Service<'_>,
    fleet: Machines<'_>,
    project: String,
    machine_id: String,
    commit: String,
    only: Vec<String>,
    lockfiles: bool,
    context: Option<u32>,
) -> Result<DiffView> {
    let place = location(&beep, &fleet, &project, Some(&machine_id)).await?;
    let shell = PeerShell::on(&fleet, place.machine)
        .await?
        .complete_output();
    let id = CommitId::from_str(&commit)?;
    let mut parts = history::run(
        &shell,
        &history::patch_context_script(&place.folder, &id, &only, context.unwrap_or(3)),
    )
    .await?;
    let patch = parts.remove("patch").ok_or_else(|| {
        DesktopError::BadRequest("Git did not return the commit diff.".to_owned())
    })?;
    let root = parts.remove("root").unwrap_or(place.folder);
    let root = root.strip_suffix('\n').unwrap_or(&root).to_owned();
    let parent = parts
        .get("parents")
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_owned();
    let ask = DiffAsk {
        root,
        base: parent,
        head: commit,
        only,
        lockfiles,
        context: 3,
    };
    Ok(view(beep_node::agents::parse_git_patch(&ask, &patch)))
}
