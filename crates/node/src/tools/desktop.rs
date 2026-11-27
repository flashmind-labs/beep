//! The home assistant's project tools, which a Beep desktop answers. A
//! desktop answers them itself through the hook its host gives `Node`, and
//! any other node sends the call to a desktop of its mesh over `tools/1` as
//! `Tool::Projects`, with the tool's action in its arguments.

use super::{Node, ProjectTools};
use crate::agents::reply::fail;
use crate::peer_stream::{elsewhere, forward};
use beep_proto::node::tools::{Reply, Request};
use std::sync::Arc;

/// The reply for a call from a session on this node.
pub(super) async fn call(node: &Node, request: &Request) -> Reply {
    if let Some(projects) = &node.project_tools {
        return projects.call(request.args.clone()).await;
    }

    let Some(mesh) = node.mesh.get() else {
        return fail(format!("{} It is not in a mesh.", elsewhere("this machine")));
    };

    forward(&mesh, request).await
}

/// This machine's own answer to a node's request, which only a desktop has.
pub(crate) async fn answer(projects: Option<&Arc<dyn ProjectTools>>, request: &Request) -> Reply {
    match projects {
        Some(projects) => projects.call(request.args.clone()).await,
        None => fail("This machine does not answer the project tools, only the Beep desktop does."),
    }
}

#[cfg(test)]
#[path = "../tests/tools_desktop.rs"]
mod tests;
