//! Giving a project's branch a worktree on one of the caller's machines,
//! from the Branches panel or for the branches an import picked.

use super::{
    Reply,
    own_nodes::online,
    reviews::{places, project_row},
};
use crate::{
    imports::checkout_for_worktree,
    projects::branches::{add_worktree, branch_name},
    protocol::{AddBranchWorktree, ApiError, BranchPlace, ImportedProject, ProjectBranch},
    router::Cx,
};

/// The most worktrees one import adds.
const MAX_IMPORT_WORKTREES: usize = 20;
/// What the default branch is taken to be until the checkout says.
const FALLBACK_DEFAULT: &str = "main";

pub(super) async fn add_branch_worktree(
    cx: Cx,
    request: AddBranchWorktree,
) -> Reply<ProjectBranch> {
    let branch = branch_name(&request.branch)
        .ok_or_else(|| ApiError::bad_request("That is not a branch name."))?;
    let project = project_row(&cx, &request.project).await?;
    let name = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.endpoint == request.machine_id)
        .map(|node| node.name)
        .ok_or_else(|| ApiError::bad_request("That machine is not one of yours."))?;
    let project_name = project.name.to_string();
    let folder = match places(&cx, &project)
        .await?
        .into_iter()
        .find(|place| place.endpoint == request.machine_id)
    {
        Some(place) => Some(place.folder),
        None => checkout_for_worktree(&cx.app, &cx.user, &project_name, &request.machine_id)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?,
    };
    let folder = folder.ok_or_else(|| {
        ApiError::bad_request(format!("{name} has no checkout of {project_name}."))
    })?;
    let node = online(&cx, &request.machine_id)
        .ok_or_else(|| ApiError::bad_request(format!("{name} is offline.")))?;
    add_worktree(&node, &folder, branch, &project.default_branch)
        .await
        .map_err(|why| ApiError::bad_request(format!("{name}: {why}")))
}

/// Adds the worktrees an import asked for, each on its machine (the one
/// it was imported on when it names none). One that fails is logged: the
/// import stands either way.
pub(super) async fn add_import_worktrees(
    cx: &Cx,
    imported: &ImportedProject,
    worktrees: &[BranchPlace],
) {
    for place in worktrees.iter().take(MAX_IMPORT_WORKTREES) {
        let endpoint = place.machine_id.as_deref().unwrap_or(&imported.machine_id);
        let Some(branch) = branch_name(&place.branch) else {
            log::info!("import: not a branch name for a worktree");
            continue;
        };
        let folder = if endpoint == imported.machine_id {
            Some(imported.folder.clone())
        } else {
            match checkout_for_worktree(&cx.app, &cx.user, &imported.project, endpoint).await {
                Ok(folder) => folder,
                Err(e) => {
                    log::info!("import: no checkout for a worktree; node={endpoint}: {e}");
                    None
                }
            }
        };
        let (Some(folder), Some(node)) = (folder, online(cx, endpoint)) else {
            log::info!("import: worktree skipped, no checkout online; node={endpoint}");
            continue;
        };
        if let Err(why) = add_worktree(&node, &folder, branch, FALLBACK_DEFAULT).await {
            log::info!("import: worktree not added; node={endpoint}: {why}");
        }
    }
}
