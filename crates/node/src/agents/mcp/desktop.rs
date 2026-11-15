//! The tools a Beep desktop answers: `plugins`, which run in the desktop, and
//! the assistant's `projects`. A desktop answers them itself through the hook
//! its host gives `Offers`, and any other node sends the call to a desktop of
//! its mesh over `tools/1`.

use super::reply::fail;
use crate::link::Offers;
use anyhow::Result;
use beep_mesh::{DeviceKind, Mesh};
use beep_proto::node::tools::{Reply, Request, Tool};
use futures::future::BoxFuture;
use std::sync::Arc;

/// What the desktop app gives its node to answer the tools only a desktop
/// answers: `Tool::Plugins` and `Tool::Projects`.
pub trait DesktopTools: Send + Sync {
    /// The reply to one call of `tool`, from the tool's arguments as JSON text.
    fn call(&self, tool: Tool, args: String) -> BoxFuture<'_, Reply>;
}

/// The reply for a call from a session on this node.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    if let Some(desktop) = &offers.desktop {
        return desktop.call(request.tool, request.args.clone()).await;
    }

    let Some(mesh) = offers.mesh.get() else {
        return fail(format!(
            "{} It is not in a mesh.",
            elsewhere(request.tool, "this machine")
        ));
    };

    forward(&mesh, request).await
}

/// This machine's own answer to a node's request, which only a desktop has.
pub(crate) async fn answer(desktop: Option<&Arc<dyn DesktopTools>>, request: &Request) -> Reply {
    match desktop {
        Some(desktop) => desktop.call(request.tool, request.args.clone()).await,
        None => fail(format!(
            "This machine does not answer {}, only the Beep desktop does.",
            request.tool
        )),
    }
}

fn elsewhere(tool: Tool, machine: &str) -> String {
    format!("The {tool} tool runs in the Beep desktop; this session runs on {machine}.")
}

/// Sends the call to the first online desktop of the mesh. A phone is a
/// desktop that only observes, so it is skipped.
async fn forward(mesh: &Mesh, request: &Request) -> Reply {
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
            elsewhere(request.tool, here)
        ));
    };
    let name = &desktop.device.name;
    let replied = super::over_the_mesh(mesh, desktop.device.id, request).await;

    from_desktop(name, request.tool, replied)
}

/// The reply of the desktop `name` to a call of `tool`.
fn from_desktop(name: &str, tool: Tool, replied: Result<Reply>) -> Reply {
    match replied {
        // A desktop from before the tool answers that it serves no such tool.
        Ok(Reply::Done { text, failed: true }) if text.contains("does not serve Unknown") => {
            fail(format!(
                "{name}: That desktop's Beep is too old for the {tool} tool. Update it."
            ))
        }
        Ok(reply) => reply,
        Err(e) => fail(format!("Could not reach the desktop {name}: {e:#}")),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_desktop.rs"]
mod tests;
