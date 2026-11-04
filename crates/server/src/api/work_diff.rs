//! The diff viewer for a session's checkout or a project's branch; the node runs git.

use super::{
    Reply,
    agent_control::live_host_back,
    own_nodes::online,
    reviews::{places, project_row},
};
use crate::{
    api::router::Cx,
    nodes::NodeWork,
    protocol::{ApiError, GetBranchDiff, GetWorkDiff, WorkDiff, node::agent::DiffAsk},
};
use beep_projects::model::branch_name;

fn unreachable_node(machine: &str, e: &anyhow::Error) -> ApiError {
    ApiError::bad_request(format!(
        "Could not read the changes on {machine}: {e:#}. Updating beep-node there may help."
    ))
}

pub(super) async fn get_work_diff(cx: Cx, request: GetWorkDiff) -> Reply<WorkDiff> {
    let live = live_host_back(&cx, &request.machine_id).await?;
    let machine = &live.host.machine_name;
    let ask = DiffAsk {
        root: request.root,
        base: request.base,
        head: request.head,
        only: request.only,
        lockfiles: request.lockfiles,
    };
    match live
        .online
        .work_diff(ask)
        .await
        .map_err(|e| unreachable_node(machine, &e))?
    {
        NodeWork::Done(diff) => Ok(diff),
        NodeWork::Refused(message) => Err(ApiError::bad_request(format!("{machine}: {message}"))),
    }
}

pub(super) async fn get_branch_diff(cx: Cx, request: GetBranchDiff) -> Reply<WorkDiff> {
    let branch = branch_name(&request.branch)
        .ok_or_else(|| {
            ApiError::bad_request(format!("{} is not a branch name.", request.branch.trim()))
        })?
        .to_owned();
    let project = project_row(&cx, &request.project).await?;
    let (node, folder) = places(&cx, &project)
        .await?
        .into_iter()
        .find_map(|place| online(&cx, &place.endpoint).map(|node| (node, place.folder)))
        .ok_or_else(|| {
            ApiError::bad_request("No machine with a checkout of the project is online.")
        })?;
    let ask = DiffAsk {
        root: folder,
        base: project.default_branch.clone(),
        head: branch,
        only: request.only,
        lockfiles: request.lockfiles,
    };
    match node
        .work_diff(ask)
        .await
        .map_err(|e| unreachable_node(&node.name, &e))?
    {
        NodeWork::Done(diff) => Ok(diff),
        NodeWork::Refused(message) => {
            Err(ApiError::bad_request(format!("{}: {message}", node.name)))
        }
    }
}
