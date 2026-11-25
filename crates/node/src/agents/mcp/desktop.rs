//! The home assistant's project tools, which a Beep desktop answers. A
//! desktop answers them itself through the hook its host gives `Offers`, and
//! any other node sends the call to a desktop of its mesh over `tools/1` as
//! `Tool::Projects`, with the tool's action in its arguments.

use super::reply::fail;
use crate::peer_stream::{CallFailed, over_the_mesh};
use crate::link::Offers;
use anyhow::Result;
use beep_mesh::{DeviceKind, Mesh};
use beep_proto::node::tools::{Reply, Request};
use futures::future::BoxFuture;
use std::sync::Arc;

/// What the desktop app gives its node to answer `Tool::Projects`.
pub trait ProjectTools: Send + Sync {
    /// The reply to one call, from its arguments as JSON text with the
    /// action under `action`.
    fn call(&self, args: String) -> BoxFuture<'_, Reply>;
}

/// The reply for a call from a session on this node.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    if let Some(projects) = &offers.project_tools {
        return projects.call(request.args.clone()).await;
    }

    let Some(mesh) = offers.mesh.get() else {
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

fn elsewhere(machine: &str) -> String {
    format!("The project tools run in the Beep desktop; this session runs on {machine}.")
}

/// Sends the call to the first online desktop of the mesh. A phone is a
/// desktop that only observes, so it is skipped.
pub(crate) async fn forward(mesh: &Mesh, request: &Request) -> Reply {
    let me = mesh.me();
    let devices = mesh.devices();
    let here = devices
        .iter()
        .find(|status| status.device.id == me)
        .map_or("a node", |status| status.device.name.as_str());
    let mut desktops: Vec<_> = devices
        .iter()
        .filter(|status| {
            status.device.kind == DeviceKind::Desktop
                && status.online
                && !mesh.is_observer(status.device.id)
        })
        .collect();
    desktops.sort_by(|a, b| a.device.name.cmp(&b.device.name));

    let Some(desktop) = desktops.first() else {
        return fail(format!(
            "{} No desktop of the mesh is online.",
            elsewhere(here)
        ));
    };
    let name = &desktop.device.name;
    let replied = over_the_mesh(mesh, desktop.device.id, request).await;

    from_desktop(name, replied)
}

/// The reply of the desktop `name`.
fn from_desktop(name: &str, replied: Result<Reply, CallFailed>) -> Reply {
    match replied {
        // A desktop from before the tool answers that it serves no such tool.
        Ok(Reply::Done { text, failed: true }) if text.contains("does not serve Unknown") => {
            fail(format!(
                "{name}: That desktop's Beep is too old for the project tools. Update it."
            ))
        }
        Ok(reply) => reply,
        Err(e) => fail(format!("Could not reach the desktop {name}: {e:#}")),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_desktop.rs"]
mod tests;
