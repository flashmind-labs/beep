//! Newer `gh`, `teacli` and `flsh` for the caller's machines, and installing them.

use super::Reply;
use crate::{
    nodes::Online,
    protocol::{
        node::{
            harness::{is_newer, parse_version},
            tool_update::Tool,
        },
        *,
    },
    router::Cx,
};

/// The tools on the caller's online machines that are behind the newest
/// release, on nodes that can install it.
pub(super) async fn list_tool_updates(cx: Cx, _: ListToolUpdates) -> Reply<Vec<ToolUpdate>> {
    let mut updates = Vec::new();
    for node in cx.user.nodes().await? {
        let Some(env) = node.env else { continue };
        let Some(online) = own_online(&cx, &node.endpoint) else {
            continue;
        };
        for tool in Tool::ALL.into_iter().filter(|&tool| online.installs(tool)) {
            let name: &str = tool.into();
            let Some(version) = env
                .tools
                .iter()
                .find(|found| found.name == name)
                .and_then(|found| parse_version(found.version.as_deref()?))
            else {
                continue;
            };
            let Some(latest) = cx.app.nodes.releases.latest(tool).await else {
                continue;
            };
            if is_newer(&latest, &version) {
                updates.push(ToolUpdate {
                    machine_id: node.endpoint.clone(),
                    tool: name.to_owned(),
                    version,
                    latest,
                });
            }
        }
    }
    Ok(updates)
}

/// Installs the newest release of `tool` on one of the caller's machines.
pub(super) async fn update_machine_tool(
    cx: Cx,
    UpdateMachineTool { machine_id, tool }: UpdateMachineTool,
) -> Reply<String> {
    let tool: Tool = tool
        .parse()
        .map_err(|_| ApiError::bad_request(format!("Beep does not update {tool}.")))?;
    let node = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.endpoint == machine_id)
        .ok_or(ApiError::NotFound {})?;
    let online = own_online(&cx, &machine_id)
        .ok_or_else(|| ApiError::bad_request(format!("{} is offline.", node.name)))?;
    if !online.installs(tool) {
        return Err(ApiError::bad_request(format!(
            "Update beep-node on {} first.",
            node.name
        )));
    }
    let latest =
        cx.app.nodes.releases.latest(tool).await.ok_or_else(|| {
            ApiError::bad_request(format!("Could not look up the newest {tool}."))
        })?;
    online.install_tool(tool, &latest).await.map_err(|e| {
        log::info!("tool not updated; node={machine_id} tool={tool} error={e:#}");
        ApiError::bad_request(format!("Could not update {tool}: {e:#}"))
    })?;
    log::info!("tool updated; node={machine_id} tool={tool} to={latest}");
    Ok(latest)
}

/// The caller's machine `machine_id`, when it is connected now.
fn own_online(cx: &Cx, machine_id: &str) -> Option<Online> {
    cx.app
        .nodes
        .online(machine_id)
        .filter(|online| online.user == cx.user.id())
}
