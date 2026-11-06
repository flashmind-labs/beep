//! Newer `gh`, `teacli` and `flsh` for the caller's machines: a desktop installs them now.

use super::{Reply, own_nodes::retired};
use crate::{api::router::Cx, protocol::*};

pub(super) async fn list_tool_updates(_: Cx, _: ListToolUpdates) -> Reply<Vec<ToolUpdate>> {
    Ok(Vec::new())
}

pub(super) async fn update_machine_tool(_: Cx, _: UpdateMachineTool) -> Reply<String> {
    retired()
}
