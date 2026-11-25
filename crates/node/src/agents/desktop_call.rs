//! Sending a call to a desktop of the mesh, the only machine that answers the
//! home assistant's project tools.

use super::reply::fail;
use crate::peer_stream::{CallFailed, over_the_mesh};
use beep_mesh::{DeviceKind, Mesh};
use beep_proto::node::tools::{Reply, Request};

pub(crate) fn elsewhere(machine: &str) -> String {
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
pub(crate) fn from_desktop(name: &str, replied: Result<Reply, CallFailed>) -> Reply {
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
