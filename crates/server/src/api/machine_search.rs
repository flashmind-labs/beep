//! Where a machine looks for git checkouts.

use super::Reply;
use crate::{api::router::Cx, nodes::Online, protocol::*};

/// Where `machine_id` looks for git checkouts.
pub(super) async fn get_machine_search(
    cx: Cx,
    GetMachineSearch { machine_id }: GetMachineSearch,
) -> Reply<String> {
    let online = search_node(&cx, &machine_id).await?;
    online
        .search()
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))
}

/// Has `machine_id` look for git checkouts in `search` and walk it again.
pub(super) async fn set_machine_search(
    cx: Cx,
    SetMachineSearch { machine_id, search }: SetMachineSearch,
) -> Reply<String> {
    let online = search_node(&cx, &machine_id).await?;
    online
        .set_search(search.trim())
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))
}

/// One of the caller's online machines that runs `search/1`.
async fn search_node(cx: &Cx, machine_id: &str) -> Result<Online, ApiError> {
    let (name, online) = owned_online(cx, machine_id).await?;
    if !online.runs_search() {
        return Err(ApiError::bad_request(format!(
            "Update beep-node on {name} to pick where it looks for projects."
        )));
    }
    Ok(online)
}

/// One of the caller's online machines, with its name. Another account's
/// machine is `NotFound`, as one that does not exist.
pub(super) async fn owned_online(cx: &Cx, machine_id: &str) -> Result<(String, Online), ApiError> {
    let owned = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.endpoint == machine_id)
        .ok_or(ApiError::NotFound {})?;
    let online = cx
        .app
        .nodes
        .online(machine_id)
        .filter(|online| online.user == cx.user.id())
        .ok_or_else(|| ApiError::bad_request(format!("{} is offline.", owned.name)))?;
    Ok((owned.name, online))
}
