//! The `plugins` tool. Plugins run in the Beep desktop, so a desktop answers
//! it itself through the hook its host gives `Offers`, and any other node
//! sends the call to a desktop of its mesh over `tools/1`.

use super::reply::fail;
use crate::link::Offers;
use beep_mesh::{DeviceKind, Mesh};
use beep_proto::node::tools::{Reply, Request};
use futures::future::BoxFuture;
use std::sync::Arc;

/// What the desktop app gives its node to answer the `plugins` tool.
pub trait PluginTools: Send + Sync {
    /// The reply to one call, from the tool's arguments as JSON text.
    fn call(&self, args: String) -> BoxFuture<'_, Reply>;
}

/// Shown when no desktop answers.
const OLD_DESKTOP: &str = "That desktop's Beep is too old for the plugins tool. Update it.";

/// The reply for a `plugins` call from a session on this node.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    if let Some(plugins) = &offers.plugins {
        return plugins.call(request.args.clone()).await;
    }

    let Some(mesh) = offers.mesh.get() else {
        return fail(format!("{} It is not in a mesh.", elsewhere("this machine")));
    };

    forward(&mesh, request).await
}

/// The reply of this machine's own plugins to a node's request, which only a
/// desktop has.
pub(crate) async fn answer(plugins: Option<&Arc<dyn PluginTools>>, request: &Request) -> Reply {
    match plugins {
        Some(plugins) => plugins.call(request.args.clone()).await,
        None => fail("This machine runs no plugins, only the Beep desktop does."),
    }
}

fn elsewhere(machine: &str) -> String {
    format!("Plugins run in the Beep desktop; this session runs on {machine}.")
}

/// Sends the call to the first online desktop of the mesh.
async fn forward(mesh: &Mesh, request: &Request) -> Reply {
    let me = mesh.me();
    let devices = mesh.devices();
    let here = devices
        .iter()
        .find(|status| status.device.id == me)
        .map_or("a node", |status| status.device.name.as_str());
    let mut desktops: Vec<_> = devices
        .iter()
        .filter(|status| status.device.kind == DeviceKind::Desktop && status.online)
        .collect();
    desktops.sort_by(|a, b| a.device.name.cmp(&b.device.name));

    let Some(desktop) = desktops.first() else {
        return fail(format!(
            "{} No desktop of the mesh is online.",
            elsewhere(here)
        ));
    };
    let name = &desktop.device.name;

    match super::over_the_mesh(mesh, desktop.device.id, request).await {
        // A desktop from before the tool answers that it serves no such tool.
        Ok(Reply::Done { text, failed: true }) if text.contains("does not serve Unknown") => {
            fail(format!("{name}: {OLD_DESKTOP}"))
        }
        Ok(reply) => reply,
        Err(e) => fail(format!("Could not reach the desktop {name}: {e:#}")),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_plugins.rs"]
mod tests;
